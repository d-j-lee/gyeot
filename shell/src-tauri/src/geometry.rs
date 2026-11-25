//! 창 배치와 클릭 영역 계산(순수 함수). 모든 화면 좌표는 물리 픽셀이다.

/// 얼굴이 알려 준 클릭 영역(웹뷰 기준 CSS px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// CSS 좌표의 상한. 이보다 큰 값은 잘라 낸다(잘못된 값이 정수 넘침을 만들지 않게).
const MAX_CSS: f64 = 32_768.0;

impl HitRect {
    /// 값이 이상하거나(무한·NaN) 넓이가 0이면 `None` — "어디도 클릭을 받지 않음"이다.
    pub fn sanitize(x: f64, y: f64, width: f64, height: f64) -> Option<Self> {
        if !(x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite()) {
            return None;
        }
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        Some(Self {
            x: x.clamp(-MAX_CSS, MAX_CSS),
            y: y.clamp(-MAX_CSS, MAX_CSS),
            width: width.min(MAX_CSS),
            height: height.min(MAX_CSS),
        })
    }

    /// 웹뷰 왼쪽 위(`origin`, 물리 px)와 배율로 화면 물리 좌표 사각형을 만든다.
    /// 가장자리는 바깥으로 반올림해 1px 틈이 생기지 않게 한다.
    pub fn to_screen(self, origin: (i32, i32), scale: f64) -> ScreenRect {
        let s = sane_scale(scale);
        let ox = f64::from(origin.0);
        let oy = f64::from(origin.1);
        ScreenRect {
            left: ox + (self.x * s).floor(),
            top: oy + (self.y * s).floor(),
            right: ox + ((self.x + self.width) * s).ceil(),
            bottom: oy + ((self.y + self.height) * s).ceil(),
        }
    }
}

/// 화면 물리 좌표 사각형(오른쪽·아래는 포함하지 않음).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl ScreenRect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// 모니터의 작업 영역(작업 표시줄을 뺀 곳), 물리 px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn sane_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// 논리 크기 → 물리 크기(반올림, 최소 1).
pub fn physical_size(logical: (f64, f64), scale: f64) -> (u32, u32) {
    let s = sane_scale(scale);
    let conv = |v: f64| (v * s).round().clamp(1.0, f64::from(u16::MAX)) as u32;
    (conv(logical.0), conv(logical.1))
}

/// 작업 영역보다 크면 줄인다.
pub fn fit_size(size: (u32, u32), wa: WorkArea) -> (u32, u32) {
    (size.0.min(wa.width.max(1)), size.1.min(wa.height.max(1)))
}

/// 작업 영역의 오른쪽 아래에 붙인 창의 왼쪽 위 좌표.
pub fn bottom_right(wa: WorkArea, size: (u32, u32)) -> (i32, i32) {
    let x = i64::from(wa.x) + i64::from(wa.width) - i64::from(size.0);
    let y = i64::from(wa.y) + i64::from(wa.height) - i64::from(size.1);
    let clamp = |v: i64, min: i32| {
        v.max(i64::from(min))
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    (clamp(x, wa.x), clamp(y, wa.y))
}

/// 커질 때는 먼저 옮기고(왼쪽 위로) 키우고, 작아질 때는 먼저 줄이고 옮긴다.
/// 그래야 중간 단계에서도 창이 작업 영역 밖(작업 표시줄 위)으로 나가지 않는다.
pub fn move_before_resize(old: (u32, u32), new: (u32, u32)) -> bool {
    u64::from(new.0) * u64::from(new.1) > u64::from(old.0) * u64::from(old.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_garbage_and_empty() {
        assert_eq!(HitRect::sanitize(f64::NAN, 0.0, 10.0, 10.0), None);
        assert_eq!(HitRect::sanitize(0.0, 0.0, f64::INFINITY, 10.0), None);
        assert_eq!(HitRect::sanitize(0.0, 0.0, 0.0, 10.0), None);
        assert_eq!(HitRect::sanitize(0.0, 0.0, 10.0, -1.0), None);
        let r = HitRect::sanitize(-1e9, 5.0, 1e9, 20.0).unwrap();
        assert_eq!(r.x, -32_768.0);
        assert_eq!(r.width, 32_768.0);
    }

    #[test]
    fn hit_rect_maps_with_scale_and_origin() {
        // 150% 배율, 웹뷰가 화면 (1000, 400)에 있을 때 CSS (200,300)-(296,396)
        let r = HitRect::sanitize(200.0, 300.0, 96.0, 96.0).unwrap();
        let s = r.to_screen((1000, 400), 1.5);
        assert_eq!(
            s,
            ScreenRect {
                left: 1300.0,
                top: 850.0,
                right: 1444.0,
                bottom: 994.0
            }
        );
        assert!(s.contains(1300.0, 850.0));
        assert!(s.contains(1443.9, 993.9));
        assert!(!s.contains(1444.0, 900.0));
        assert!(!s.contains(1299.9, 900.0));
    }

    #[test]
    fn hit_rect_rounds_outward_on_fractional_scale() {
        let r = HitRect::sanitize(10.0, 10.0, 10.0, 10.0).unwrap();
        let s = r.to_screen((0, 0), 1.25);
        assert_eq!((s.left, s.top, s.right, s.bottom), (12.0, 12.0, 25.0, 25.0));
    }

    #[test]
    fn negative_origin_on_left_monitor() {
        let r = HitRect::sanitize(0.0, 0.0, 50.0, 50.0).unwrap();
        let s = r.to_screen((-1920, 0), 1.0);
        assert!(s.contains(-1900.0, 10.0));
        assert!(!s.contains(10.0, 10.0));
    }

    #[test]
    fn bad_scale_falls_back_to_one() {
        assert_eq!(physical_size((360.0, 520.0), 0.0), (360, 520));
        assert_eq!(physical_size((360.0, 520.0), f64::NAN), (360, 520));
        assert_eq!(physical_size((360.0, 520.0), 1.25), (450, 650));
        assert_eq!(physical_size((420.0, 680.0), 1.75), (735, 1190));
    }

    #[test]
    fn anchors_bottom_right_of_work_area() {
        // 1920x1080, 아래 작업 표시줄 48px
        let wa = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1032,
        };
        assert_eq!(bottom_right(wa, (360, 520)), (1560, 512));
        // 왼쪽 작업 표시줄, 보조 모니터 오프셋
        let wa = WorkArea {
            x: 2000,
            y: -200,
            width: 1000,
            height: 700,
        };
        assert_eq!(bottom_right(wa, (360, 520)), (2640, -20));
    }

    #[test]
    fn oversize_window_is_fitted_and_kept_inside() {
        let wa = WorkArea {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        let size = fit_size(physical_size((420.0, 680.0), 1.0), wa);
        assert_eq!(size, (420, 600));
        assert_eq!(bottom_right(wa, size), (380, 0));
        // fit_size 를 거치지 않은 큰 창도 작업 영역 왼쪽 위를 넘지 않는다
        assert_eq!(bottom_right(wa, (1000, 900)), (0, 0));
    }

    #[test]
    fn resize_order() {
        assert!(move_before_resize((360, 520), (420, 680)));
        assert!(!move_before_resize((420, 680), (360, 520)));
        assert!(!move_before_resize((360, 520), (360, 520)));
    }
}
