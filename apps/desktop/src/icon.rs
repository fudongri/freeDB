use eframe::egui::IconData;
use image::{ImageBuffer, Rgba, RgbaImage};
use std::path::Path;

const BG_TOP: [u8; 4] = [39, 57, 126, 255];
const BG_MID: [u8; 4] = [58, 37, 119, 255];
const BG_BOTTOM: [u8; 4] = [20, 152, 166, 255];
const PANEL_GLOW: [u8; 4] = [255, 255, 255, 30];
const PANEL_EDGE: [u8; 4] = [255, 255, 255, 56];
const HORSE_FILL: [u8; 4] = [245, 249, 255, 255];
const HORSE_ACCENT: [u8; 4] = [136, 219, 255, 210];
const HORSE_SHADOW: [u8; 4] = [103, 131, 205, 110];
const SPARK: [u8; 4] = [227, 245, 255, 235];

pub fn app_icon_data(size: u32) -> IconData {
    let image = render_icon(size);
    IconData {
        rgba: image.into_raw(),
        width: size,
        height: size,
    }
}

pub fn export_icon_png(path: impl AsRef<Path>, size: u32) -> Result<(), image::ImageError> {
    render_icon(size).save(path)
}

fn render_icon(size: u32) -> RgbaImage {
    let mut image: RgbaImage = ImageBuffer::from_pixel(size, size, Rgba([0, 0, 0, 0]));
    let s = size as f32;

    fill_rounded_rect_gradient(
        &mut image,
        rect(0.07 * s, 0.07 * s, 0.86 * s, 0.86 * s),
        0.22 * s,
        BG_TOP,
        BG_MID,
        BG_BOTTOM,
    );
    stroke_rounded_rect(
        &mut image,
        rect(0.105 * s, 0.105 * s, 0.79 * s, 0.79 * s),
        0.18 * s,
        2.0_f32.max(s * 0.002),
        PANEL_EDGE,
    );
    fill_rounded_rect_solid(
        &mut image,
        rect(0.17 * s, 0.14 * s, 0.66 * s, 0.18 * s),
        0.05 * s,
        PANEL_GLOW,
    );

    fill_circle(
        &mut image,
        point(0.52 * s, 0.53 * s),
        0.26 * s,
        [255, 255, 255, 24],
    );
    fill_ellipse_gradient(
        &mut image,
        point(0.53 * s, 0.54 * s),
        0.24 * s,
        0.28 * s,
        [170, 225, 255, 55],
        [0, 0, 0, 0],
    );

    let horse_shape = [
        point(0.36 * s, 0.79 * s),
        point(0.29 * s, 0.60 * s),
        point(0.31 * s, 0.44 * s),
        point(0.39 * s, 0.30 * s),
        point(0.49 * s, 0.21 * s),
        point(0.60 * s, 0.18 * s),
        point(0.68 * s, 0.22 * s),
        point(0.72 * s, 0.12 * s),
        point(0.80 * s, 0.10 * s),
        point(0.79 * s, 0.25 * s),
        point(0.85 * s, 0.34 * s),
        point(0.86 * s, 0.43 * s),
        point(0.82 * s, 0.54 * s),
        point(0.74 * s, 0.63 * s),
        point(0.67 * s, 0.67 * s),
        point(0.69 * s, 0.75 * s),
        point(0.57 * s, 0.74 * s),
        point(0.49 * s, 0.64 * s),
        point(0.44 * s, 0.72 * s),
    ];
    fill_polygon(&mut image, &horse_shape, HORSE_FILL);

    let mane_shape = [
        point(0.39 * s, 0.76 * s),
        point(0.35 * s, 0.56 * s),
        point(0.39 * s, 0.38 * s),
        point(0.48 * s, 0.25 * s),
        point(0.58 * s, 0.21 * s),
        point(0.52 * s, 0.36 * s),
        point(0.49 * s, 0.50 * s),
        point(0.52 * s, 0.66 * s),
    ];
    fill_polygon(&mut image, &mane_shape, HORSE_ACCENT);

    let jaw_shadow = [
        point(0.59 * s, 0.34 * s),
        point(0.76 * s, 0.35 * s),
        point(0.81 * s, 0.43 * s),
        point(0.76 * s, 0.51 * s),
        point(0.67 * s, 0.57 * s),
        point(0.60 * s, 0.51 * s),
    ];
    fill_polygon(&mut image, &jaw_shadow, HORSE_SHADOW);

    stroke_line(
        &mut image,
        point(0.45 * s, 0.66 * s),
        point(0.60 * s, 0.30 * s),
        0.012 * s,
        [255, 255, 255, 105],
    );
    stroke_line(
        &mut image,
        point(0.60 * s, 0.30 * s),
        point(0.73 * s, 0.24 * s),
        0.012 * s,
        [255, 255, 255, 95],
    );
    fill_circle(&mut image, point(0.67 * s, 0.34 * s), 0.018 * s, SPARK);
    fill_circle(&mut image, point(0.78 * s, 0.43 * s), 0.01 * s, [71, 94, 148, 180]);

    image
}

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Clone, Copy)]
struct Point {
    x: f32,
    y: f32,
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

fn point(x: f32, y: f32) -> Point {
    Point { x, y }
}

fn fill_rounded_rect_solid(image: &mut RgbaImage, rect: Rect, radius: f32, color: [u8; 4]) {
    let x0 = rect.x.max(0.0).floor() as u32;
    let y0 = rect.y.max(0.0).floor() as u32;
    let x1 = (rect.x + rect.w).ceil().min(image.width() as f32) as u32;
    let y1 = (rect.y + rect.h).ceil().min(image.height() as f32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            if rounded_rect_contains(x as f32 + 0.5, y as f32 + 0.5, rect, radius) {
                blend_pixel(image, x, y, color);
            }
        }
    }
}

fn fill_rounded_rect_gradient(
    image: &mut RgbaImage,
    rect: Rect,
    radius: f32,
    top: [u8; 4],
    mid: [u8; 4],
    bottom: [u8; 4],
) {
    let x0 = rect.x.max(0.0).floor() as u32;
    let y0 = rect.y.max(0.0).floor() as u32;
    let x1 = (rect.x + rect.w).ceil().min(image.width() as f32) as u32;
    let y1 = (rect.y + rect.h).ceil().min(image.height() as f32) as u32;
    for y in y0..y1 {
        let t = ((y as f32 + 0.5 - rect.y) / rect.h).clamp(0.0, 1.0);
        let color = if t < 0.58 {
            lerp_color(top, mid, t / 0.58)
        } else {
            lerp_color(mid, bottom, (t - 0.58) / 0.42)
        };
        for x in x0..x1 {
            if rounded_rect_contains(x as f32 + 0.5, y as f32 + 0.5, rect, radius) {
                blend_pixel(image, x, y, color);
            }
        }
    }
}

fn stroke_rounded_rect(
    image: &mut RgbaImage,
    bounds: Rect,
    radius: f32,
    stroke: f32,
    color: [u8; 4],
) {
    let outer = bounds;
    let inner = rect(
        bounds.x + stroke,
        bounds.y + stroke,
        bounds.w - stroke * 2.0,
        bounds.h - stroke * 2.0,
    );
    let x0 = bounds.x.max(0.0).floor() as u32;
    let y0 = bounds.y.max(0.0).floor() as u32;
    let x1 = (bounds.x + bounds.w).ceil().min(image.width() as f32) as u32;
    let y1 = (bounds.y + bounds.h).ceil().min(image.height() as f32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            if rounded_rect_contains(px, py, outer, radius)
                && !rounded_rect_contains(px, py, inner, (radius - stroke).max(0.0))
            {
                blend_pixel(image, x, y, color);
            }
        }
    }
}

fn fill_ellipse_solid(image: &mut RgbaImage, center: Point, rx: f32, ry: f32, color: [u8; 4]) {
    let x0 = (center.x - rx).floor().max(0.0) as u32;
    let y0 = (center.y - ry).floor().max(0.0) as u32;
    let x1 = (center.x + rx).ceil().min(image.width() as f32) as u32;
    let y1 = (center.y + ry).ceil().min(image.height() as f32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let nx = (x as f32 + 0.5 - center.x) / rx;
            let ny = (y as f32 + 0.5 - center.y) / ry;
            if nx * nx + ny * ny <= 1.0 {
                blend_pixel(image, x, y, color);
            }
        }
    }
}

fn fill_ellipse_gradient(
    image: &mut RgbaImage,
    center: Point,
    rx: f32,
    ry: f32,
    top: [u8; 4],
    bottom: [u8; 4],
) {
    let x0 = (center.x - rx).floor().max(0.0) as u32;
    let y0 = (center.y - ry).floor().max(0.0) as u32;
    let x1 = (center.x + rx).ceil().min(image.width() as f32) as u32;
    let y1 = (center.y + ry).ceil().min(image.height() as f32) as u32;
    for y in y0..y1 {
        let t = ((y as f32 + 0.5 - (center.y - ry)) / (ry * 2.0)).clamp(0.0, 1.0);
        let color = lerp_color(top, bottom, t);
        for x in x0..x1 {
            let nx = (x as f32 + 0.5 - center.x) / rx;
            let ny = (y as f32 + 0.5 - center.y) / ry;
            if nx * nx + ny * ny <= 1.0 {
                blend_pixel(image, x, y, color);
            }
        }
    }
}

fn fill_circle(image: &mut RgbaImage, center: Point, radius: f32, color: [u8; 4]) {
    fill_ellipse_solid(image, center, radius, radius, color);
}

fn fill_polygon(image: &mut RgbaImage, points: &[Point], color: [u8; 4]) {
    if points.len() < 3 {
        return;
    }
    let min_x = points
        .iter()
        .map(|point| point.x)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as u32;
    let min_y = points
        .iter()
        .map(|point| point.y)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as u32;
    let max_x = points
        .iter()
        .map(|point| point.x)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(image.width() as f32) as u32;
    let max_y = points
        .iter()
        .map(|point| point.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(image.height() as f32) as u32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            if polygon_contains(points, x as f32 + 0.5, y as f32 + 0.5) {
                blend_pixel(image, x, y, color);
            }
        }
    }
}

fn polygon_contains(points: &[Point], x: f32, y: f32) -> bool {
    let mut inside = false;
    let mut previous = *points.last().expect("polygon contains at least 3 points");
    for current in points {
        let intersects = ((current.y > y) != (previous.y > y))
            && (x
                < (previous.x - current.x) * (y - current.y)
                    / ((previous.y - current.y).abs().max(f32::EPSILON))
                    + current.x);
        if intersects {
            inside = !inside;
        }
        previous = *current;
    }
    inside
}

fn stroke_line(image: &mut RgbaImage, a: Point, b: Point, thickness: f32, color: [u8; 4]) {
    let min_x = (a.x.min(b.x) - thickness).floor().max(0.0) as u32;
    let min_y = (a.y.min(b.y) - thickness).floor().max(0.0) as u32;
    let max_x = (a.x.max(b.x) + thickness).ceil().min(image.width() as f32) as u32;
    let max_y = (a.y.max(b.y) + thickness).ceil().min(image.height() as f32) as u32;
    let line_dx = b.x - a.x;
    let line_dy = b.y - a.y;
    let line_len_sq = line_dx * line_dx + line_dy * line_dy;
    if line_len_sq <= f32::EPSILON {
        return;
    }
    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let t = (((px - a.x) * line_dx) + ((py - a.y) * line_dy)) / line_len_sq;
            let t = t.clamp(0.0, 1.0);
            let proj_x = a.x + line_dx * t;
            let proj_y = a.y + line_dy * t;
            let dx = px - proj_x;
            let dy = py - proj_y;
            if dx * dx + dy * dy <= thickness * thickness {
                blend_pixel(image, x, y, color);
            }
        }
    }
}

fn rounded_rect_contains(x: f32, y: f32, rect: Rect, radius: f32) -> bool {
    let left = rect.x;
    let right = rect.x + rect.w;
    let top = rect.y;
    let bottom = rect.y + rect.h;
    if x < left || x > right || y < top || y > bottom {
        return false;
    }
    let r = radius.min(rect.w * 0.5).min(rect.h * 0.5);
    let cx = x.clamp(left + r, right - r);
    let cy = y.clamp(top + r, bottom - r);
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

fn lerp_color(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let mut out = [0_u8; 4];
    for i in 0..4 {
        out[i] = (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    }
    out
}

fn blend_pixel(image: &mut RgbaImage, x: u32, y: u32, color: [u8; 4]) {
    let dst = image.get_pixel_mut(x, y);
    let alpha = color[3] as f32 / 255.0;
    let inv = 1.0 - alpha;
    let blended = [
        (color[0] as f32 * alpha + dst[0] as f32 * inv).round() as u8,
        (color[1] as f32 * alpha + dst[1] as f32 * inv).round() as u8,
        (color[2] as f32 * alpha + dst[2] as f32 * inv).round() as u8,
        ((color[3] as f32) + dst[3] as f32 * inv).round().min(255.0) as u8,
    ];
    *dst = Rgba(blended);
}
