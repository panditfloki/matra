// Standalone native-controller replay. No Tauri, OS capture or external crates.
#[path = "../matra/src/carry.rs"]
mod carry;
use carry::{BorderTrack, Controller, Edge, Metrics, Point};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scale: f64 = args[1].parse().unwrap();
    let layouts: Vec<Vec<f64>> = args[2..6]
        .iter()
        .map(|a| a.split(',').map(|s| s.parse().unwrap()).collect())
        .collect();
    let track = BorderTrack::new(1200.0, 900.0).unwrap();
    let metrics = std::array::from_fn(|i| Metrics {
        length: layouts[i][0] * scale,
        depth: layouts[i][1] * scale,
        trailing: layouts[i][2] * scale,
    });
    let mut c = Controller::new(
        track,
        metrics,
        Edge::Right,
        450.0,
        Point {
            x: 1195.0,
            y: 450.0,
        },
    );
    let print = |c: &Controller| {
        let s = c.shape();
        let a = &layouts[s.from.index()];
        let b = &layouts[s.to.index()];
        let mix = |a: f64, b: f64| a + (b - a) * s.turned;
        let rings = (5..a.len())
            .map(|i| {
                let offset =
                    mix(a[i] + a[4] - a[0] / 2.0, b[i] + b[4] - b[0] / 2.0) * scale + s.back;
                let p = c.track.ring_point(
                    c.track.wrapped(s.place + offset),
                    mix(a[3], b[3]) * scale - 2.0,
                );
                format!("[{:.6},{:.6}]", p.x, p.y)
            })
            .collect::<Vec<_>>()
            .join(",");
        let corner = s
            .round
            .map(|r| format!("\"{}\"", r.corner.name()))
            .unwrap_or_else(|| "null".into());
        println!("{{\"id\":1,\"w\":1200,\"h\":900,\"place\":{:.6},\"length\":{:.6},\"turned\":{:.6},\"corner\":{},\"before\":{:.6},\"after\":{:.6},\"from\":\"{}\",\"to\":\"{}\",\"thin\":{:.6},\"back\":{:.6},\"rings\":[{}],\"settling\":{}}}",s.place,s.length,s.turned,corner,s.round.map_or(s.length,|r|r.before),s.round.map_or(0.0,|r|r.after),s.from.name(),s.to.name(),s.thin,s.back,rings,c.phase!=carry::Phase::Dragging);
    };
    print(&c);
    for n in 1..=500 {
        let (edge, along) = track.place(1650.0 + n as f64 * track.perimeter() / 450.0);
        let p = match edge {
            Edge::Top => Point { x: along, y: 5.0 },
            Edge::Right => Point {
                x: 1195.0,
                y: along,
            },
            Edge::Bottom => Point { x: along, y: 895.0 },
            Edge::Left => Point { x: 5.0, y: along },
        };
        c.pointer(p);
        c.tick(1.0 / 120.0);
        print(&c);
    }
    c.release();
    for _ in 0..180 {
        c.tick(1.0 / 120.0);
        print(&c);
        if c.phase == carry::Phase::Finished {
            break;
        }
    }
}
