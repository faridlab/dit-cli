//! The menu bar icon: the DIT mark — a stem, a branch, three nodes — as a
//! template image (black, shaped by alpha), so macOS tints it for light and
//! dark menu bars. Drawn here from its geometry rather than decoded from a
//! picture, which keeps an image decoder out of the app.

/// The mark's shapes, in the 512-unit square of `apps/web/src/assets/dit-logo.png`.
const NODES: [(f32, f32); 3] = [(170.0, 105.0), (340.0, 277.0), (170.0, 405.0)];
const NODE_RADIUS: f32 = 64.0;
const STROKE: f32 = 21.0; // half the stem's width
const ARC_CENTER: (f32, f32) = (255.0, 192.0);
const ARC_RADIUS: f32 = 85.0;

/// RGBA pixels, `size` × `size`. `strength` scales the alpha: 1.0 while
/// DIT runs, lower while it is stopped.
pub fn mark(size: u32, strength: f32) -> Vec<u8> {
    let scale = 512.0 / size as f32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let px = (x as f32 + 0.5) * scale;
            let py = (y as f32 + 0.5) * scale;
            // Signed distance in mark units, turned into pixel coverage.
            let d = distance(px, py) / scale;
            let coverage = (0.5 - d).clamp(0.0, 1.0) * strength.clamp(0.0, 1.0);
            rgba.extend_from_slice(&[0, 0, 0, (coverage * 255.0).round() as u8]);
        }
    }
    rgba
}

/// Distance from a point to the mark's edge; negative inside.
fn distance(x: f32, y: f32) -> f32 {
    let nodes = NODES
        .iter()
        .map(|&(cx, cy)| hypot(x - cx, y - cy) - NODE_RADIUS)
        .fold(f32::MAX, f32::min);
    let stem = segment(x, y, NODES[0], NODES[2]) - STROKE;
    // The branch leaves the stem in a quarter turn, then runs to the node.
    let (ax, ay) = ARC_CENTER;
    let on_arc = if x <= ax && y >= ay {
        (hypot(x - ax, y - ay) - ARC_RADIUS).abs()
    } else {
        f32::MAX
    };
    let arc = on_arc - STROKE;
    let branch = segment(x, y, (ax, ay + ARC_RADIUS), NODES[1]) - STROKE;
    nodes.min(stem).min(arc).min(branch)
}

fn segment(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    hypot(x - (a.0 + t * dx), y - (a.1 + t * dy))
}

fn hypot(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn alpha(rgba: &[u8], size: u32, x: u32, y: u32) -> u8 {
        rgba[((y * size + x) * 4 + 3) as usize]
    }

    #[test]
    fn the_mark_fills_its_nodes_and_leaves_the_corners_clear() {
        let size = 36;
        let rgba = mark(size, 1.0);
        assert_eq!(rgba.len(), (size * size * 4) as usize);
        // The top node's centre, and the empty top-right corner.
        assert_eq!(alpha(&rgba, size, 12, 7), 255);
        assert_eq!(alpha(&rgba, size, 34, 1), 0);
        // Template images are black; only alpha carries the shape.
        assert!(rgba.chunks(4).all(|p| p[..3] == [0, 0, 0]));
    }

    #[test]
    fn a_stopped_server_dims_the_mark() {
        let bright = mark(36, 1.0);
        let dim = mark(36, 0.4);
        assert_eq!(alpha(&dim, 36, 12, 7), 102);
        assert!(
            bright.iter().map(|&b| b as u32).sum::<u32>()
                > dim.iter().map(|&b| b as u32).sum::<u32>()
        );
    }
}
