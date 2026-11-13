//! Dependency-free perimeter carry, ported from the preserved Mac BorderTrack and
//! NotchWindowController. All geometry is in work-area-local logical points.
//! DPI is applied only at the native boundary, never to the 40/160 point rules.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

impl Edge {
    pub const ALL: [Self; 4] = [Self::Top, Self::Right, Self::Bottom, Self::Left];
    pub fn parse(s: &str) -> Self {
        match s {
            "top" => Self::Top,
            "bottom" => Self::Bottom,
            "left" => Self::Left,
            _ => Self::Right,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Right => "right",
            Self::Bottom => "bottom",
            Self::Left => "left",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Right => 1,
            Self::Bottom => 2,
            Self::Left => 3,
        }
    }
    pub fn vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
    pub fn sign(self) -> f64 {
        if matches!(self, Self::Top | Self::Right) {
            1.0
        } else {
            -1.0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopRight,
    BottomRight,
    BottomLeft,
    TopLeft,
}
impl Corner {
    pub const ALL: [Self; 4] = [
        Self::TopRight,
        Self::BottomRight,
        Self::BottomLeft,
        Self::TopLeft,
    ];
    pub fn edges(self) -> (Edge, Edge) {
        match self {
            Self::TopRight => (Edge::Top, Edge::Right),
            Self::BottomRight => (Edge::Right, Edge::Bottom),
            Self::BottomLeft => (Edge::Bottom, Edge::Left),
            Self::TopLeft => (Edge::Left, Edge::Top),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::TopRight => "topRight",
            Self::BottomRight => "bottomRight",
            Self::BottomLeft => "bottomLeft",
            Self::TopLeft => "topLeft",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderTrack {
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Passage {
    pub corner: Corner,
    pub before: f64,
    pub after: f64,
}
impl BorderTrack {
    pub fn new(width: f64, height: f64) -> Option<Self> {
        (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
            .then_some(Self { width, height })
    }
    pub fn perimeter(self) -> f64 {
        2.0 * (self.width + self.height)
    }
    pub fn wrapped(self, at: f64) -> f64 {
        at.rem_euclid(self.perimeter())
    }
    pub fn delta(self, from: f64, to: f64) -> f64 {
        let d = (to - from) % self.perimeter();
        if d > self.perimeter() / 2.0 {
            d - self.perimeter()
        } else if d < -self.perimeter() / 2.0 {
            d + self.perimeter()
        } else {
            d
        }
    }
    pub fn corner_position(self, corner: Corner) -> f64 {
        match corner {
            Corner::TopRight => self.width,
            Corner::BottomRight => self.width + self.height,
            Corner::BottomLeft => 2.0 * self.width + self.height,
            Corner::TopLeft => 0.0,
        }
    }
    pub fn position(self, edge: Edge, p: Point) -> f64 {
        match edge {
            Edge::Top => p.x.clamp(0.0, self.width),
            Edge::Right => self.width + p.y.clamp(0.0, self.height),
            Edge::Bottom => 2.0 * self.width + self.height - p.x.clamp(0.0, self.width),
            Edge::Left => self.wrapped(self.perimeter() - p.y.clamp(0.0, self.height)),
        }
    }
    pub fn place(self, position: f64) -> (Edge, f64) {
        let s = self.wrapped(position);
        if s < self.width {
            (Edge::Top, s)
        } else if s < self.width + self.height {
            (Edge::Right, s - self.width)
        } else if s < 2.0 * self.width + self.height {
            (Edge::Bottom, 2.0 * self.width + self.height - s)
        } else {
            (Edge::Left, self.perimeter() - s)
        }
    }
    pub fn on(self, edge: Edge, along: f64) -> f64 {
        self.position(edge, Point { x: along, y: along })
    }
    pub fn span(self, edge: Edge) -> f64 {
        if edge.vertical() {
            self.height
        } else {
            self.width
        }
    }
    pub fn corner(self, position: f64, length: f64) -> Option<Passage> {
        // A body longer than an edge cannot wrap multiple corners faithfully.
        // The renderer clips its content; constrain the shape before splitting.
        let length = length.min(self.width.min(self.height) * 2.0);
        Corner::ALL.into_iter().find_map(|corner| {
            let d = self.delta(position, self.corner_position(corner));
            (d.abs() < length / 2.0).then_some(Passage {
                corner,
                before: d + length / 2.0,
                after: length / 2.0 - d,
            })
        })
    }
    pub fn distance(self, edge: Edge, p: Point) -> f64 {
        match edge {
            Edge::Top => p.y.max(0.0),
            Edge::Right => (self.width - p.x).max(0.0),
            Edge::Bottom => (self.height - p.y).max(0.0),
            Edge::Left => p.x.max(0.0),
        }
    }
    pub fn sticky_edge(self, current: Edge, p: Point) -> Edge {
        let nearest = Edge::ALL
            .into_iter()
            .min_by(|a, b| self.distance(*a, p).total_cmp(&self.distance(*b, p)))
            .unwrap();
        if self.distance(nearest, p) + 40.0 < self.distance(current, p) {
            nearest
        } else {
            current
        }
    }
    pub fn reading(self, p: Point, nearest: Edge) -> Reading {
        for c in Corner::ALL {
            let (a, b) = c.edges();
            if self.distance(a, p) < 160.0 && self.distance(b, p) < 160.0 {
                return Reading::Corner(c);
            }
        }
        Reading::Edge(nearest)
    }
    pub fn read(self, p: Point, reading: Reading) -> f64 {
        match reading {
            Reading::Edge(e) => self.position(e, p),
            Reading::Corner(c) => {
                let (a, b) = c.edges();
                self.wrapped(self.corner_position(c) + self.distance(a, p) - self.distance(b, p))
            }
        }
    }
    pub fn rest(self, at: f64, metrics: &[Metrics; 4]) -> f64 {
        let (e, along) = self.place(at);
        let m = metrics[e.index()];
        self.on(e, clamp_center(along, self.span(e), m.length, m.trailing))
    }
    /// The same inset cubic as CornerPassageView.ringPoint. Rings remain upright.
    pub fn ring_point(self, at: f64, inset: f64) -> Point {
        let d = inset.max(0.0);
        let round = 1.6 * d;
        let point = |s| {
            let (e, a) = self.place(s);
            match e {
                Edge::Top => Point { x: a, y: d },
                Edge::Right => Point {
                    x: self.width - d,
                    y: a,
                },
                Edge::Bottom => Point {
                    x: a,
                    y: self.height - d,
                },
                Edge::Left => Point { x: d, y: a },
            }
        };
        for c in Corner::ALL {
            let off = self.delta(self.corner_position(c), at);
            if round <= 0.0 || off.abs() >= round {
                continue;
            }
            let a = point(self.corner_position(c) - round);
            let b = point(self.corner_position(c) + round);
            let heading = |e| match e {
                Edge::Top => Point { x: 1.0, y: 0.0 },
                Edge::Right => Point { x: 0.0, y: 1.0 },
                Edge::Bottom => Point { x: -1.0, y: 0.0 },
                Edge::Left => Point { x: 0.0, y: -1.0 },
            };
            let (first, second) = c.edges();
            let da = heading(first);
            let db = heading(second);
            let k = (round - d) * 0.8;
            let c1 = Point {
                x: a.x + da.x * k,
                y: a.y + da.y * k,
            };
            let c2 = Point {
                x: b.x - db.x * k,
                y: b.y - db.y * k,
            };
            let u = (off + round) / (2.0 * round);
            let v = 1.0 - u;
            return Point {
                x: v * v * v * a.x
                    + 3.0 * v * v * u * c1.x
                    + 3.0 * v * u * u * c2.x
                    + u * u * u * b.x,
                y: v * v * v * a.y
                    + 3.0 * v * v * u * c1.y
                    + 3.0 * v * u * u * c2.y
                    + u * u * u * b.y,
            };
        }
        point(at)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    Edge(Edge),
    Corner(Corner),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Complete body, including both flares.
    pub length: f64,
    pub depth: f64,
    /// Handles beyond the trailing flare.
    pub trailing: f64,
}

pub fn clamp_center(along: f64, span: f64, length: f64, trailing: f64) -> f64 {
    let low = length / 2.0;
    let high = span - length / 2.0 - trailing;
    if low <= high {
        along.clamp(low, high)
    } else {
        span / 2.0
    }
}

/// Existing settings store fractions of the work span, never Mac point offsets.
pub fn fraction(along: f64, span: f64) -> f64 {
    (along / span.max(1.0)).clamp(0.0, 1.0)
}

/// Separate the window's viewport from the actual visible body. The viewport stays
/// in the work area where possible; an explicit local anchor places the body inside
/// it. This removes the old 650-point viewport clamp without clipping hover cards.
pub fn anchored_window(
    span: f64,
    viewport: f64,
    ratio: f64,
    length: f64,
    trailing: f64,
) -> (f64, f64) {
    let middle = clamp_center(span * ratio.clamp(0.0, 1.0), span, length, trailing);
    let origin = (middle - viewport / 2.0).clamp(0.0, (span - viewport).max(0.0));
    (origin, middle - origin)
}

/// Signed desktop physical coordinates become local points before any distance test.
/// Effective logical size when a viewport is constrained by a narrow display.
/// fitZoom scales the entire CSS design to the measured physical viewport width.
pub fn layout_scale(viewport_width: f64, design_width: f64, dpi: f64) -> Option<f64> {
    if ![viewport_width, design_width, dpi]
        .into_iter()
        .all(|n| n.is_finite() && n > 0.0)
    {
        return None;
    }
    Some(viewport_width / design_width / dpi)
}

pub fn local_point(physical: Point, work_origin: Point, dpi: f64) -> Option<Point> {
    if ![physical.x, physical.y, work_origin.x, work_origin.y, dpi]
        .into_iter()
        .all(f64::is_finite)
        || dpi <= 0.0
    {
        return None;
    }
    Some(Point {
        x: (physical.x - work_origin.x) / dpi,
        y: (physical.y - work_origin.y) / dpi,
    })
}

pub fn spring(
    gap: f64,
    mut velocity: f64,
    elapsed: f64,
    response: f64,
    damping: f64,
) -> (f64, f64) {
    let omega = 2.0 * std::f64::consts::PI / response;
    let mut moved = 0.0;
    let dt = elapsed.clamp(0.0, 1.0 / 30.0) / 2.0;
    for _ in 0..2 {
        velocity += (omega * omega * (gap - moved) - 2.0 * damping * omega * velocity) * dt;
        moved += velocity * dt;
    }
    (moved, velocity)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Dragging,
    Settling,
    Finished,
    Cancelled,
}
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub place: f64,
    pub length: f64,
    pub turned: f64,
    pub round: Option<Passage>,
    pub from: Edge,
    pub to: Edge,
    pub thin: f64,
    pub back: f64,
}

pub struct Controller {
    pub track: BorderTrack,
    pub metrics: [Metrics; 4],
    pub phase: Phase,
    pub shown: f64,
    pub target: f64,
    pub velocity: f64,
    pub start_edge: Edge,
    pointer_edge: Edge,
    reading: Reading,
    grip: f64,
}
impl Controller {
    pub fn new(
        track: BorderTrack,
        metrics: [Metrics; 4],
        edge: Edge,
        body_along: f64,
        pointer: Point,
    ) -> Self {
        let reading = track.reading(pointer, edge);
        let shown = track.on(edge, body_along);
        Self {
            track,
            metrics,
            phase: Phase::Dragging,
            shown,
            target: shown,
            velocity: 0.0,
            start_edge: edge,
            pointer_edge: edge,
            reading,
            grip: track.delta(track.read(pointer, reading), shown),
        }
    }
    pub fn pointer(&mut self, p: Point) {
        if self.phase != Phase::Dragging {
            return;
        }
        let edge = self.track.sticky_edge(self.pointer_edge, p);
        let next = self.track.reading(p, edge);
        if next != self.reading {
            self.grip += self
                .track
                .delta(self.track.read(p, next), self.track.read(p, self.reading));
            self.reading = next;
        }
        self.pointer_edge = edge;
        self.target = self
            .track
            .wrapped(self.track.read(p, self.reading) + self.grip);
    }
    pub fn cross_screen(&mut self, track: BorderTrack, p: Point) {
        if self.phase != Phase::Dragging {
            return;
        }
        self.track = track;
        self.reading = track.reading(p, self.pointer_edge);
        self.target = track.wrapped(track.read(p, self.reading) + self.grip);
        self.shown = self.target;
        self.velocity = 0.0;
    }
    pub fn shape(&self) -> Shape {
        let stretch = if self.phase == Phase::Finished {
            0.0
        } else {
            0.14 * (self.velocity.abs() / 2600.0).min(1.0)
        };
        let resting = self.metrics[self.track.place(self.shown).0.index()].length;
        let back = if self.velocity >= 0.0 { 1.0 } else { -1.0 } * resting * stretch / 2.0;
        let place = self.track.wrapped(self.shown - back);
        let e = self.track.place(place).0;
        let mut length = self.metrics[e.index()].length * (1.0 + stretch);
        let mut round = self.track.corner(place, length);
        let mut turned = 0.0;
        if let Some(r) = round {
            let (first, second) = r.corner.edges();
            for _ in 0..2 {
                if let Some(r) = round {
                    let t = (r.after / length.max(1.0)).clamp(0.0, 1.0);
                    turned = t * t * (3.0 - 2.0 * t);
                    length = (self.metrics[first.index()].length
                        + (self.metrics[second.index()].length
                            - self.metrics[first.index()].length)
                            * turned)
                        * (1.0 + stretch);
                    round = self.track.corner(place, length);
                }
            }
        }
        let (from, to) = round.map(|r| r.corner.edges()).unwrap_or((e, e));
        Shape {
            place,
            length,
            turned,
            round,
            from,
            to,
            thin: 1.0 - stretch * 0.35,
            back,
        }
    }
    pub fn release(&mut self) {
        if self.phase != Phase::Dragging {
            return;
        }
        let shape = self.shape();
        let mut target = self.target;
        if let Some(r) = shape.round {
            let (first, second) = r.corner.edges();
            let onto = if r.before >= r.after { first } else { second };
            let half = self.metrics[onto.index()].length / 2.0 + 1.0;
            target = self.track.wrapped(
                self.track.corner_position(r.corner) + if onto == first { -half } else { half },
            );
        }
        self.target = self.track.rest(target, &self.metrics);
        self.phase = Phase::Settling;
    }
    /// The final ordinary layout may have a different measured body on a shorter
    /// monitor. Reconcile it while still hidden, before the last settling glide.
    pub fn measured_landing(&mut self, edge: Edge, metrics: Metrics) {
        self.metrics[edge.index()] = metrics;
        self.target = self.track.rest(
            self.track.on(edge, self.track.place(self.target).1),
            &self.metrics,
        );
    }
    pub fn tick(&mut self, elapsed: f64) -> bool {
        if matches!(self.phase, Phase::Finished | Phase::Cancelled) {
            return true;
        }
        let settling = self.phase == Phase::Settling;
        let gap = self.track.delta(self.shown, self.target);
        if gap.abs() < if settling { 0.3 } else { 0.05 }
            && self.velocity.abs() < if settling { 6.0 } else { 1.0 }
        {
            self.velocity = 0.0;
            self.shown = self.target;
            if settling {
                self.phase = Phase::Finished;
                return true;
            }
            return false;
        }
        let (moved, velocity) = spring(
            gap,
            self.velocity,
            elapsed,
            if settling { 0.3 } else { 0.2 },
            if settling { 0.74 } else { 0.84 },
        );
        self.velocity = velocity;
        self.shown = self.track.wrapped(self.shown + moved);
        false
    }
    pub fn cancel(&mut self) {
        if self.phase != Phase::Finished {
            self.phase = Phase::Cancelled;
            self.velocity = 0.0;
        }
    }
    pub fn landing(&self) -> (Edge, f64) {
        let (e, a) = self.track.place(self.target);
        (e, fraction(a, self.track.span(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t() -> BorderTrack {
        BorderTrack::new(1800.0, 1169.0).unwrap()
    }
    fn metrics() -> [Metrics; 4] {
        [Metrics {
            length: 200.0,
            depth: 70.0,
            trailing: 64.0,
        }; 4]
    }
    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-7, "{a} != {b}");
    }
    #[test]
    fn edges_round_trip_and_wrap() {
        let t = t();
        for (e, a) in [
            (Edge::Top, 400.0),
            (Edge::Right, 300.0),
            (Edge::Bottom, 700.0),
            (Edge::Left, 900.0),
        ] {
            let back = t.place(t.on(e, a));
            assert_eq!(back.0, e);
            near(back.1, a);
        }
        near(t.wrapped(-10.0), t.perimeter() - 10.0);
        near(t.wrapped(t.perimeter() + 10.0), 10.0);
    }
    #[test]
    fn mac_corner_vectors() {
        let t = t();
        assert!(t.corner(900.0, 200.0).is_none());
        let r = t.corner(1760.0, 200.0).unwrap();
        assert_eq!(r.corner, Corner::TopRight);
        near(r.before, 140.0);
        near(r.after, 60.0);
        near(t.corner(30.0, 200.0).unwrap().after, 130.0);
    }
    #[test]
    fn mac_hysteresis_vectors() {
        let t = t();
        let p = Point { x: 1780.0, y: 19.0 };
        assert_eq!(t.sticky_edge(Edge::Top, p), Edge::Top);
        assert_eq!(t.sticky_edge(Edge::Right, p), Edge::Right);
        assert_eq!(
            t.sticky_edge(
                Edge::Top,
                Point {
                    x: 1790.0,
                    y: 469.0
                }
            ),
            Edge::Right
        );
        assert_eq!(
            t.sticky_edge(
                Edge::Right,
                Point {
                    x: 1500.0,
                    y: 1154.0
                }
            ),
            Edge::Bottom
        );
        assert_eq!(
            t.sticky_edge(Edge::Bottom, Point { x: 10.0, y: 569.0 }),
            Edge::Left
        );
        assert_eq!(
            t.sticky_edge(Edge::Left, Point { x: 900.0, y: 9.0 }),
            Edge::Top
        );
    }
    #[test]
    fn switch_requires_more_than_forty() {
        let t = t();
        assert_eq!(
            t.sticky_edge(Edge::Top, Point { x: 1790.0, y: 50.0 }),
            Edge::Top
        );
        assert_eq!(
            t.sticky_edge(
                Edge::Top,
                Point {
                    x: 1790.0,
                    y: 50.01
                }
            ),
            Edge::Right
        );
    }
    #[test]
    fn corner_reading_reach_is_points() {
        let t = t();
        assert_eq!(
            t.reading(Point { x: 1790.0, y: 20.0 }, Edge::Top),
            Reading::Corner(Corner::TopRight)
        );
        assert_eq!(
            t.reading(Point { x: 1640.0, y: 10.0 }, Edge::Top),
            Reading::Edge(Edge::Top)
        );
    }
    #[test]
    fn all_corners_are_monotonic_in_both_directions() {
        let t = t();
        for c in Corner::ALL {
            let (a, b) = c.edges();
            let mut last = None;
            for off in -150..=150 {
                let at = t.corner_position(c) + off as f64;
                let (e, along) = t.place(at);
                let p = match e {
                    Edge::Top => Point { x: along, y: 0.0 },
                    Edge::Right => Point {
                        x: t.width,
                        y: along,
                    },
                    Edge::Bottom => Point {
                        x: along,
                        y: t.height,
                    },
                    Edge::Left => Point { x: 0.0, y: along },
                };
                let read = t.read(p, Reading::Corner(c));
                near(t.delta(at, read), 0.0);
                if let Some(prev) = last {
                    assert!(t.delta(prev, read) > 0.0);
                    assert!(t.delta(read, prev) < 0.0);
                }
                last = Some(read);
            }
            assert_ne!(a, b);
        }
    }
    #[test]
    fn grip_is_preserved_and_reading_switch_is_continuous() {
        let t = t();
        let mut c = Controller::new(
            t,
            metrics(),
            Edge::Top,
            1000.0,
            Point { x: 1100.0, y: 10.0 },
        );
        c.pointer(Point { x: 1200.0, y: 10.0 });
        near(c.target, 1100.0);
        c.pointer(Point { x: 1639.0, y: 10.0 });
        let old = c.target;
        c.pointer(Point { x: 1641.0, y: 10.0 });
        near(t.delta(old, c.target), 2.0);
        c.pointer(Point { x: 1750.0, y: 90.0 });
        let before = c.target;
        c.pointer(Point { x: 1750.0, y: 91.0 });
        near(t.delta(before, c.target), 1.0);
    }
    #[test]
    fn shortest_distance_across_zero() {
        let t = t();
        near(t.delta(t.perimeter() - 5.0, 5.0), 10.0);
        near(t.delta(5.0, t.perimeter() - 5.0), -10.0);
    }
    #[test]
    fn release_tie_chooses_previous_edge() {
        let t = t();
        for corner in Corner::ALL {
            let edge = corner.edges().1;
            let mut c = Controller::new(t, metrics(), edge, 0.0, Point { x: 0.0, y: 0.0 });
            c.shown = t.corner_position(corner);
            c.target = c.shown;
            c.release();
            assert_eq!(c.landing().0, corner.edges().0);
        }
    }
    #[test]
    fn release_uses_majority_then_clamps_trailing_handles() {
        let t = t();
        let mut c = Controller::new(t, metrics(), Edge::Top, 1770.0, Point { x: 1770.0, y: 0.0 });
        c.release();
        assert_eq!(c.landing().0, Edge::Top);
        near(t.place(c.target).1, 1636.0);
    }
    #[test]
    fn all_landings_fit_measured_body_and_handles() {
        let t = t();
        for e in Edge::ALL {
            for a in [-20.0, 0.1, 300.0, t.span(e) - 0.1, t.span(e) + 20.0] {
                let held = clamp_center(a, t.span(e), 200.0, 64.0);
                assert!(held >= 100.0);
                assert!(held + 100.0 + 64.0 <= t.span(e));
            }
        }
    }
    #[test]
    fn oversize_centres_without_inverted_clamp() {
        near(clamp_center(10.0, 100.0, 500.0, 80.0), 50.0);
        near(anchored_window(100.0, 650.0, 0.0, 500.0, 80.0).1, 50.0);
    }
    #[test]
    fn viewport_is_not_body_clamp() {
        let (o, a) = anchored_window(1000.0, 650.0, 0.1, 200.0, 64.0);
        near(o, 0.0);
        near(a, 100.0);
        let (o, a) = anchored_window(1000.0, 650.0, 1.0, 200.0, 64.0);
        near(o, 350.0);
        near(o + a, 836.0);
    }
    #[test]
    fn fractional_positions_survive_per_edge_restore() {
        let t = t();
        for (e, r) in [
            (Edge::Top, 0.23),
            (Edge::Right, 0.64),
            (Edge::Bottom, 0.71),
            (Edge::Left, 0.39),
        ] {
            let (o, a) = anchored_window(t.span(e), 650.0, r, 200.0, 64.0);
            near(fraction(o + a, t.span(e)), r);
        }
    }
    #[test]
    fn a_narrow_display_uses_the_measured_layout_scale() {
        near(layout_scale(575.0, 460.0, 1.0).unwrap(), 1.25);
        near(layout_scale(400.0, 460.0, 1.0).unwrap(), 400.0 / 460.0);
        near(layout_scale(650.0, 650.0, 2.0).unwrap(), 0.5);
        assert!(layout_scale(0.0, 460.0, 1.0).is_none());
        let k = layout_scale(400.0, 460.0, 1.5).unwrap() * 1.5;
        let (origin, anchor) = anchored_window(900.0, 650.0, 0.1, 200.0 * k, 64.0 * k);
        near(origin + anchor, 90.0);
    }
    #[test]
    fn negative_origin_and_mixed_dpi() {
        let p = Point {
            x: -1800.0,
            y: -100.0,
        };
        let o = Point {
            x: -2000.0,
            y: -400.0,
        };
        assert_eq!(local_point(p, o, 2.0), Some(Point { x: 100.0, y: 150.0 }));
        assert_eq!(local_point(p, o, 1.25), Some(Point { x: 160.0, y: 240.0 }));
        assert!(local_point(p, o, 0.0).is_none());
    }
    #[test]
    fn spring_matches_mac_settling_and_follow() {
        for (response, damping, min, max, settle) in
            [(0.2, 0.84, 0.0, 2.0, 0.5), (0.3, 0.74, 0.1, 5.0, 0.8)]
        {
            for fps in [60.0, 120.0] {
                let (mut p, mut v, mut peak, mut by) = (0.0, 0.0, 0.0_f64, None);
                for frame in 0..(fps * 2.0) as usize {
                    let (m, n) = spring(100.0 - p, v, 1.0 / fps, response, damping);
                    p += m;
                    v = n;
                    peak = peak.max(p);
                    if by.is_none() && (100.0 - p).abs() < 0.3 && v.abs() < 6.0 {
                        by = Some((frame + 1) as f64 / fps);
                    }
                }
                assert!(peak - 100.0 >= min && peak - 100.0 < max);
                assert!(by.unwrap() < settle);
            }
        }
    }
    #[test]
    fn slow_frame_is_capped() {
        assert_eq!(
            spring(100.0, 30.0, 5.0, 0.2, 0.84),
            spring(100.0, 30.0, 1.0 / 30.0, 0.2, 0.84)
        );
    }
    #[test]
    fn quick_release_and_duplicate_terminal_are_stable() {
        let mut c = Controller::new(
            t(),
            metrics(),
            Edge::Right,
            400.0,
            Point {
                x: 1790.0,
                y: 500.0,
            },
        );
        c.release();
        for _ in 0..180 {
            c.tick(1.0 / 60.0);
        }
        assert_eq!(c.phase, Phase::Finished);
        let target = c.target;
        c.release();
        c.cancel();
        near(c.target, target);
        assert_eq!(c.phase, Phase::Finished);
    }
    #[test]
    fn cancel_never_finishes_or_reacts_to_pointer() {
        let mut c = Controller::new(
            t(),
            metrics(),
            Edge::Right,
            400.0,
            Point {
                x: 1790.0,
                y: 500.0,
            },
        );
        c.cancel();
        let target = c.target;
        c.pointer(Point { x: 0.0, y: 0.0 });
        c.release();
        assert!(c.tick(0.016));
        assert_eq!(c.phase, Phase::Cancelled);
        near(c.target, target);
    }
    #[test]
    fn monitor_crossing_retains_logical_grip() {
        let mut c = Controller::new(
            t(),
            metrics(),
            Edge::Right,
            400.0,
            Point {
                x: 1790.0,
                y: 500.0,
            },
        );
        let other = BorderTrack::new(1200.0, 900.0).unwrap();
        c.cross_screen(
            other,
            Point {
                x: 1190.0,
                y: 600.0,
            },
        );
        near(other.place(c.target).1, 500.0);
    }
    #[test]
    fn ring_cubic_is_continuous_for_all_corners() {
        let t = t();
        for c in Corner::ALL {
            for off in [-48.0, 0.0, 48.0] {
                let a = t.ring_point(t.corner_position(c) + off - 0.001, 30.0);
                let b = t.ring_point(t.corner_position(c) + off + 0.001, 30.0);
                assert!((a.x - b.x).hypot(a.y - b.y) < 0.01);
                assert!(a.x >= 0.0 && a.y >= 0.0 && a.x <= t.width && a.y <= t.height);
            }
        }
    }
    #[test]
    fn measured_release_reclamps_before_visible_handoff() {
        let mut c = Controller::new(
            t(),
            metrics(),
            Edge::Right,
            1000.0,
            Point {
                x: 1790.0,
                y: 1000.0,
            },
        );
        c.release();
        c.measured_landing(
            Edge::Right,
            Metrics {
                length: 400.0,
                depth: 70.0,
                trailing: 100.0,
            },
        );
        near(t().place(c.target).1, 869.0);
    }
}
