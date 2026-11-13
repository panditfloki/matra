//! Native carry transaction. The ordinary HWND stays visible (its DOM is hidden)
//! so capture and WebView layout survive until the measured handoff is painted.
use crate::{
    carry::{self, BorderTrack, Controller, Edge, Metrics, Phase, Point},
    AppHandle, AppState, Screen,
};
use std::sync::{
    atomic::{AtomicU64, Ordering::SeqCst},
    Mutex,
};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

static ACTIVE: AtomicU64 = AtomicU64::new(0);
static RELEASED: AtomicU64 = AtomicU64::new(0);
static CANCELLED: AtomicU64 = AtomicU64::new(0);
static BODY: Mutex<[Option<Metrics>; 4]> = Mutex::new([None; 4]);
static DESTINATION: Mutex<Option<Destination>> = Mutex::new(None);
static LAYOUT: Mutex<Option<LayoutReady>> = Mutex::new(None);

#[derive(Clone)]
pub struct Destination {
    pub screen: Screen,
    pub edge: String,
    pub ratio: f64,
}
pub fn destination() -> Option<Destination> {
    DESTINATION.lock().unwrap().clone()
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct Layout {
    pub length: f64,
    pub depth: f64,
    pub trailing: f64,
    pub centers: Vec<f64>,
    pub inset: f64,
    pub cell_shift: f64,
}
impl Layout {
    fn metrics(&self, scale: f64) -> Metrics {
        Metrics {
            length: self.length * scale,
            depth: self.depth * scale,
            trailing: self.trailing * scale,
        }
    }
    fn valid(&self) -> bool {
        [
            self.length,
            self.depth,
            self.trailing,
            self.inset,
            self.cell_shift,
        ]
        .into_iter()
        .all(f64::is_finite)
            && (1.0..=10000.0).contains(&self.length)
            && (1.0..=1000.0).contains(&self.depth)
            && (0.0..=1000.0).contains(&self.trailing)
            && (0.0..=1000.0).contains(&self.inset)
            && self.cell_shift.abs() <= 1000.0
            && self.centers.len() <= 64
            && self
                .centers
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 10000.0)
    }
}
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct Cell {
    pub image: String,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct Presentation {
    /// Top, right, bottom, left, with frozen provider order and actual cell images.
    pub layouts: Vec<Layout>,
    pub cells: Vec<Cell>,
    pub source_edge: String,
    pub fill: String,
    pub stroke: String,
    pub ink: String,
    pub flare: f64,
    pub corner_radius: f64,
    pub bleed: f64,
    pub scale: f64,
    pub grip: bool,
}
impl Presentation {
    fn valid(&self) -> bool {
        self.layouts.len() == 4
            && self.layouts.iter().all(Layout::valid)
            && self.cells.len() <= 64
            && self
                .layouts
                .iter()
                .all(|l| l.centers.len() == self.cells.len())
            && self.cells.iter().all(|c| {
                c.image.starts_with("data:image/svg+xml;")
                    && c.image.len() <= 1_000_000
                    && c.width.is_finite()
                    && c.height.is_finite()
                    && (1.0..=1000.0).contains(&c.width)
                    && (1.0..=1000.0).contains(&c.height)
            })
            && self.cells.iter().map(|c| c.image.len()).sum::<usize>() <= 8_000_000
            && [&self.fill, &self.stroke, &self.ink]
                .into_iter()
                .all(|c| c.len() <= 100)
            && [self.flare, self.corner_radius, self.bleed, self.scale]
                .into_iter()
                .all(f64::is_finite)
            && (0.0..=100.0).contains(&self.flare)
            && (0.0..=100.0).contains(&self.corner_radius)
            && (0.0..=20.0).contains(&self.bleed)
    }
}
#[derive(serde::Deserialize)]
pub struct Request {
    pub id: u64,
    /// Physical client coordinates, measured at the initiating press, not after IPC.
    pub pointer: [f64; 2],
    pub body_center: [f64; 2],
    pub presentation: Presentation,
}

#[derive(Clone, serde::Serialize)]
pub struct Frame {
    pub id: u64,
    pub w: f64,
    pub h: f64,
    pub place: f64,
    pub length: f64,
    pub turned: f64,
    pub corner: Option<String>,
    pub before: f64,
    pub after: f64,
    pub from: String,
    pub to: String,
    pub thin: f64,
    pub back: f64,
    pub rings: Vec<[f64; 2]>,
    pub settling: bool,
}
fn frame(id: u64, c: &Controller, p: &Presentation) -> Frame {
    let shape = c.shape();
    let from = &p.layouts[shape.from.index()];
    let to = &p.layouts[shape.to.index()];
    let mix = |a: f64, b: f64| a + (b - a) * shape.turned;
    let sign = c.start_edge.sign();
    let scale = p.scale;
    let inset = mix(from.inset, to.inset) * scale - p.bleed;
    let rings = from
        .centers
        .iter()
        .zip(&to.centers)
        .map(|(a, b)| {
            let offset =
                sign * mix(
                    a + from.cell_shift - from.length / 2.0,
                    b + to.cell_shift - to.length / 2.0,
                ) * scale
                    + shape.back;
            let at = c
                .track
                .ring_point(c.track.wrapped(shape.place + offset), inset);
            [at.x, at.y]
        })
        .collect();
    Frame {
        id,
        w: c.track.width,
        h: c.track.height,
        place: shape.place,
        length: shape.length,
        turned: shape.turned,
        corner: shape.round.map(|r| r.corner.name().into()),
        before: shape.round.map_or(shape.length, |r| r.before),
        after: shape.round.map_or(0.0, |r| r.after),
        from: shape.from.name().into(),
        to: shape.to.name().into(),
        thin: shape.thin,
        back: shape.back,
        rings,
        settling: c.phase != Phase::Dragging,
    }
}

#[derive(Clone, serde::Serialize)]
struct Terminal {
    id: u64,
    status: &'static str,
    reason: Option<String>,
}
/// One owner for every terminal path, including a panic or window destruction.
struct Session {
    app: AppHandle,
    id: u64,
    captured: bool,
    status: &'static str,
    reason: Option<String>,
}
impl Session {
    fn release_capture(&mut self) -> Result<(), String> {
        if self.captured {
            self.captured = false;
            crate::platform::carry::end(&self.app)?;
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Err(e) = self.release_capture() {
            self.status = "cancelled";
            self.reason = Some(e);
        }
        *DESTINATION.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // Config was untouched on cancellation; restore the previous reachable placement.
        crate::place_notch(&self.app);
        let event = Terminal {
            id: self.id,
            status: self.status,
            reason: self.reason.clone(),
        };
        let _ = self.app.emit_to("notch", "carry_end", event);
        // Retain the final overlay until the ordinary DOM has painted again.
        let until = Instant::now() + Duration::from_millis(250);
        while Instant::now() < until && crate::dropzones::revealed() != self.id {
            if self.app.get_webview_window("notch").is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        crate::dropzones::hide(&self.app);
        *LAYOUT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if ACTIVE.compare_exchange(self.id, 0, SeqCst, SeqCst).is_ok() {
            crate::DRAGGING.store(false, SeqCst);
        }
        crate::place_notch(&self.app);
        crate::topmost::restore(&self.app);
    }
}

pub fn begin(app: AppHandle, mut request: Request) -> Result<u64, String> {
    if request.id == 0
        || request.id > 9_007_199_254_740_991
        || !request.presentation.valid()
        || !request
            .pointer
            .into_iter()
            .chain(request.body_center)
            .all(|n| n.is_finite() && n.abs() < 100_000.0)
    {
        return Err("Invalid measured carry geometry".into());
    }
    ACTIVE
        .compare_exchange(0, request.id, SeqCst, SeqCst)
        .map_err(|_| "A notch carry is already active".to_string())?;
    crate::DRAGGING.store(true, SeqCst);
    request.presentation.scale = crate::ui_scale(&app);
    let id = request.id;
    std::thread::spawn(move || {
        let mut session = Session {
            app,
            id,
            captured: false,
            status: "cancelled",
            reason: None,
        };
        if let Err(error) = run(&mut session, request) {
            crate::applog(&format!("notch carry {id}: {error}"));
            session.reason = Some(error);
        }
    });
    Ok(id)
}

/// A quick mouse-up can reach IPC before the worker or begin IPC. Tokens retain it.
#[tauri::command]
pub fn carry_pointer_end(id: u64, cancel: bool) {
    let active = ACTIVE.load(SeqCst);
    if id == 0 || (active != 0 && active != id) {
        return;
    }
    // Frontend ids are monotonic, including a backwards wall-clock adjustment.
    // An older queued terminal signal cannot replace a newer quick release.
    if cancel {
        CANCELLED.fetch_max(id, SeqCst);
    } else {
        RELEASED.fetch_max(id, SeqCst);
    }
}

#[derive(Clone)]
struct LayoutReady {
    id: u64,
    stage: u8,
    center: [f64; 2],
    layout: Layout,
}
#[tauri::command]
pub fn carry_layout_ready(id: u64, stage: u8, center: [f64; 2], layout: Layout) {
    if ACTIVE.load(SeqCst) == id
        && (stage == 1 || stage == 2)
        && center.into_iter().all(f64::is_finite)
        && layout.valid()
    {
        *LAYOUT.lock().unwrap() = Some(LayoutReady {
            id,
            stage,
            center,
            layout,
        });
    }
}

#[tauri::command]
pub fn report_notch_body(app: AppHandle, edge: String, layout: Layout) {
    if !layout.valid() {
        return;
    }
    let e = Edge::parse(&edge);
    let metric = layout.metrics(1.0);
    let changed = {
        let mut body = BODY.lock().unwrap();
        let old = body[e.index()];
        body[e.index()] = Some(metric);
        old.map(|m| {
            (m.length - metric.length).abs() > 0.1 || (m.trailing - metric.trailing).abs() > 0.1
        })
        .unwrap_or(true)
    };
    if changed && !crate::DRAGGING.load(SeqCst) {
        crate::place_notch(&app);
    }
}

/// The viewport stays inside the work area; the measured body moves inside it.
pub fn placement(
    screen: &Screen,
    edge: &str,
    ww: i32,
    wh: i32,
    ratio: f64,
    size: f64,
) -> Option<(i32, i32, f64)> {
    let e = Edge::parse(edge);
    let metric = BODY.lock().unwrap()[e.index()]?;
    let (ax, ay, aw, ah) = screen.area();
    let k = carry::layout_scale(ww as f64, crate::notch_window_size(edge).0, screen.scale)
        .map(|s| s * screen.scale)
        .unwrap_or(screen.scale * size)
        .max(0.01);
    let (span, viewport) = if e.vertical() {
        (ah as f64, wh as f64)
    } else {
        (aw as f64, ww as f64)
    };
    let (origin, anchor) = carry::anchored_window(
        span,
        viewport,
        ratio,
        metric.length * k,
        metric.trailing * k,
    );
    let (x, y) = match e {
        Edge::Left => (ax, ay + origin.round() as i32),
        Edge::Right => (ax + aw - ww, ay + origin.round() as i32),
        Edge::Top => (ax + origin.round() as i32, ay),
        Edge::Bottom => (ax + origin.round() as i32, ay + ah - wh),
    };
    Some((x, y, (origin + anchor - origin.round()) / k))
}

fn track(screen: &Screen) -> Result<BorderTrack, String> {
    let (_, _, w, h) = screen.area();
    BorderTrack::new(w as f64 / screen.scale, h as f64 / screen.scale)
        .ok_or_else(|| "Display has no usable work area".into())
}
fn local(screen: &Screen, p: Point) -> Result<Point, String> {
    let (x, y, _, _) = screen.area();
    carry::local_point(
        p,
        Point {
            x: x as f64,
            y: y as f64,
        },
        screen.scale,
    )
    .ok_or_else(|| "Invalid display scale or pointer".into())
}
fn same_layout(a: &Screen, b: &Screen) -> bool {
    crate::same_screen(a, b)
        && a.work == b.work
        && (a.scale - b.scale).abs() < 0.001
        && a.name == b.name
}
fn cancelled(session: &Session) -> Result<(), String> {
    if CANCELLED.load(SeqCst) == session.id {
        return Err("Carry cancelled".into());
    }
    if session.app.get_webview_window("notch").is_none() {
        return Err("Notch window closed".into());
    }
    Ok(())
}
fn attached(app: &AppHandle, screen: &Screen) -> bool {
    crate::screens(app).iter().any(|s| same_layout(screen, s))
}
fn wait_layout(session: &Session, stage: u8) -> Result<LayoutReady, String> {
    let until = Instant::now() + Duration::from_millis(1200);
    while Instant::now() < until {
        cancelled(session)?;
        if crate::platform::carry::sample(&session.app)?.escape_down {
            return Err("Carry cancelled".into());
        }
        if let Some(v) = LAYOUT
            .lock()
            .unwrap()
            .as_ref()
            .filter(|v| v.id == session.id && v.stage == stage)
            .cloned()
        {
            return Ok(v);
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    Err("The notch did not confirm its measured landing".into())
}
fn ask_layout(session: &Session, stage: u8) -> Result<LayoutReady, String> {
    *LAYOUT.lock().unwrap() = None;
    crate::place_notch(&session.app);
    session
        .app
        .emit_to(
            "notch",
            "carry_layout",
            serde_json::json!({"id":session.id,"stage":stage}),
        )
        .map_err(|e| e.to_string())?;
    wait_layout(session, stage)
}
fn destination_for(c: &Controller, screen: &Screen) -> Destination {
    let (edge, ratio) = c.landing();
    Destination {
        screen: screen.clone(),
        edge: edge.name().into(),
        ratio,
    }
}

fn run(session: &mut Session, mut request: Request) -> Result<(), String> {
    cancelled(session)?;
    if RELEASED.load(SeqCst) == session.id {
        session.status = "finished";
        return Ok(());
    }
    let app = session.app.clone();
    let notch = app
        .get_webview_window("notch")
        .ok_or("Notch window closed")?;
    let start = crate::target_screen(&app).ok_or("No display is available")?;
    let pos = notch.outer_position().map_err(|e| e.to_string())?;
    let edge = {
        let st = app.state::<AppState>();
        let c = st.cfg.lock().unwrap();
        Edge::parse(&c.notch_edge)
    };
    request.presentation.source_edge = edge.name().into();
    let requested_scale = request.presentation.scale;
    let scale = carry::layout_scale(
        notch.outer_size().map_err(|e| e.to_string())?.width as f64,
        crate::notch_window_size(edge.name()).0,
        start.scale,
    )
    .ok_or("Invalid measured notch viewport")?;
    request.presentation.scale = scale;
    request.presentation.bleed *= scale / requested_scale;
    let measured = local(
        &start,
        Point {
            x: pos.x as f64 + request.body_center[0],
            y: pos.y as f64 + request.body_center[1],
        },
    )?;
    let pointer = local(
        &start,
        Point {
            x: pos.x as f64 + request.pointer[0],
            y: pos.y as f64 + request.pointer[1],
        },
    )?;
    let metrics = std::array::from_fn(|i| request.presentation.layouts[i].metrics(scale));
    {
        let mut body = BODY.lock().unwrap();
        for (i, l) in request.presentation.layouts.iter().enumerate() {
            body[i] = Some(l.metrics(1.0));
        }
    }
    let mut controller = Controller::new(
        track(&start)?,
        metrics,
        edge,
        if edge.vertical() {
            measured.y
        } else {
            measured.x
        },
        pointer,
    );
    crate::platform::carry::begin(&app, "notch")?;
    session.captured = true;
    let first = crate::platform::carry::sample(&app)?;
    if first.escape_down || first.capture_lost {
        return Err("Carry cancelled before presentation".into());
    }
    if !first.left_down || RELEASED.load(SeqCst) == session.id {
        session.status = "finished";
        return Ok(());
    }
    crate::set_click_through(&app, false);
    crate::dropzones::show(
        &app,
        &start,
        &request.presentation,
        &frame(session.id, &controller, &request.presentation),
    )?;
    let ready_by = Instant::now() + Duration::from_millis(2000);
    while crate::dropzones::ready() != session.id {
        cancelled(session)?;
        let p = crate::platform::carry::sample(&app)?;
        if p.escape_down || p.capture_lost {
            return Err("Carry capture was lost before presentation".into());
        }
        if !p.left_down || RELEASED.load(SeqCst) == session.id {
            session.status = "finished";
            return Ok(());
        }
        if Instant::now() >= ready_by {
            return Err("Carry overlay did not paint".into());
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    app.emit_to("notch", "carry_present", session.id)
        .map_err(|e| e.to_string())?;
    let mut mon = start.clone();
    let mut last = Instant::now();
    let mut checked = Instant::now();
    let mut all = crate::screens(&app);
    loop {
        cancelled(session)?;
        if app.get_webview_window("dropzones").is_none() {
            return Err("Carry overlay closed".into());
        }
        if checked.elapsed() >= Duration::from_millis(250) {
            all = crate::screens(&app);
            checked = Instant::now();
            if !all.iter().any(|s| same_layout(&mon, s)) {
                return Err("Display geometry changed during carry".into());
            }
        }
        let pointer = crate::platform::carry::sample(&app)?;
        if pointer.escape_down || pointer.capture_lost {
            return Err("Carry cancelled or capture lost".into());
        }
        if let Some(next) = crate::screen_at(&all, pointer.x, pointer.y) {
            if !crate::same_screen(&mon, next) {
                mon = next.clone();
                controller.cross_screen(
                    track(&mon)?,
                    local(
                        &mon,
                        Point {
                            x: pointer.x,
                            y: pointer.y,
                        },
                    )?,
                );
                crate::dropzones::relocate(
                    &app,
                    &mon,
                    &frame(session.id, &controller, &request.presentation),
                )?;
            }
        }
        controller.pointer(local(
            &mon,
            Point {
                x: pointer.x,
                y: pointer.y,
            },
        )?);
        let now = Instant::now();
        let elapsed = now.duration_since(last).as_secs_f64();
        last = now;
        controller.tick(elapsed);
        crate::dropzones::retarget(&app, &frame(session.id, &controller, &request.presentation))?;
        if !pointer.left_down || RELEASED.load(SeqCst) == session.id {
            break;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    session.release_capture()?;
    if !attached(&app, &mon) {
        return Err("Destination display changed before release".into());
    }
    if !crate::same_screen(&start, &mon) && mon.name.is_none() {
        return Err("This display has no stable name; the notch was restored".into());
    }
    controller.release();
    *DESTINATION.lock().unwrap() = Some(destination_for(&controller, &mon));
    let measured = ask_layout(session, 1)?;
    let landing_edge = controller.landing().0;
    if measured.layout.centers.len() != request.presentation.cells.len() {
        return Err("Provider layout changed during carry".into());
    }
    let landing_scale = carry::layout_scale(
        notch.outer_size().map_err(|e| e.to_string())?.width as f64,
        crate::notch_window_size(landing_edge.name()).0,
        mon.scale,
    )
    .ok_or("Invalid measured landing viewport")?;
    request.presentation.bleed *= landing_scale / request.presentation.scale;
    request.presentation.scale = landing_scale;
    request.presentation.layouts[landing_edge.index()] = measured.layout.clone();
    controller.metrics =
        std::array::from_fn(|i| request.presentation.layouts[i].metrics(landing_scale));
    controller.measured_landing(landing_edge, measured.layout.metrics(landing_scale));
    BODY.lock().unwrap()[landing_edge.index()] = Some(measured.layout.metrics(1.0));
    crate::dropzones::presentation(&app, &request.presentation)?;
    *DESTINATION.lock().unwrap() = Some(destination_for(&controller, &mon));
    let settle_by = Instant::now() + Duration::from_secs(3);
    last = Instant::now();
    while controller.phase != Phase::Finished {
        cancelled(session)?;
        if crate::platform::carry::sample(&app)?.escape_down {
            return Err("Carry cancelled".into());
        }
        if !attached(&app, &mon) {
            return Err("Destination display changed while settling".into());
        }
        if Instant::now() >= settle_by {
            return Err("Carry spring did not settle".into());
        }
        let now = Instant::now();
        controller.tick(now.duration_since(last).as_secs_f64());
        last = now;
        crate::dropzones::retarget(&app, &frame(session.id, &controller, &request.presentation))?;
        std::thread::sleep(Duration::from_millis(8));
    }
    let final_layout = ask_layout(session, 2)?;
    let at = notch.outer_position().map_err(|e| e.to_string())?;
    let measured = local(
        &mon,
        Point {
            x: at.x as f64 + final_layout.center[0],
            y: at.y as f64 + final_layout.center[1],
        },
    )?;
    let (final_edge, along) = controller.track.place(controller.target);
    let actual = if final_edge.vertical() {
        measured.y
    } else {
        measured.x
    };
    if (actual - along).abs() > 1.5 {
        return Err(format!(
            "Landing body did not align ({actual:.2} versus {along:.2} points)"
        ));
    }
    if !attached(&app, &mon) {
        return Err("Destination display was removed before saving".into());
    }
    cancelled(session)?;
    let (edge, ratio) = controller.landing();
    {
        let st = app.state::<AppState>();
        let mut c = st.cfg.lock().unwrap();
        let mut next = c.clone();
        next.notch_edge = edge.name().into();
        next.set_along(edge.name(), ratio);
        next.notch_monitor = mon.name.clone();
        crate::config::save_checked(&next)?;
        *c = next;
    }
    session.status = "finished";
    crate::applog(&format!(
        "notch carried to {} at {ratio:.6} on {:?}",
        edge.name(),
        mon.name
    ));
    Ok(())
}
