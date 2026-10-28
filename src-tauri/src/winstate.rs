//! 窗口状态记忆：位置、尺寸、最大化，存于 melody-launcher/window-state.json。
//! 本模块只含纯逻辑与文件读写（不依赖 tauri），便于单元测试；与窗口的交互在 lib.rs。
use crate::config;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 物理像素坐标；width/height 为「还原（非最大化）」状态下的尺寸。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

/// 显示器矩形（物理像素）与缩放比。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

impl MonitorRect {
    fn contains(&self, px: i64, py: i64) -> bool {
        px >= self.x as i64
            && py >= self.y as i64
            && px < self.x as i64 + self.width as i64
            && py < self.y as i64 + self.height as i64
    }
}

pub fn state_path() -> PathBuf {
    config::data_dir().join("window-state.json")
}

pub fn load(p: &Path) -> Option<WindowState> {
    let t = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&t).ok()
}

pub fn save(p: &Path, s: &WindowState) -> Result<(), String> {
    let t = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    // 先写临时文件再改名，避免退出时写一半
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, t).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

/// 校验保存的状态：尺寸不得小于最小尺寸（逻辑像素 × 所在显示器缩放），
/// 标题栏区域须落在某个可见显示器内；尺寸超过显示器时收缩到显示器大小。
/// 返回 None 表示应使用默认尺寸并居中。
pub fn validate(saved: &WindowState, monitors: &[MonitorRect], min_logical: (f64, f64)) -> Option<WindowState> {
    if saved.width == 0 || saved.height == 0 || monitors.is_empty() {
        return None;
    }
    // 标题栏左上附近的点（窗口无边框，顶部 30 逻辑像素为拖动区）
    let probe_x = saved.x as i64 + (saved.width as i64).min(100) / 2;
    let probe_y = saved.y as i64 + 10;
    let m = monitors.iter().find(|m| m.contains(probe_x, probe_y))?;
    let scale = if m.scale > 0.0 { m.scale } else { 1.0 };
    let min_w = (min_logical.0 * scale).round() as u32;
    let min_h = (min_logical.1 * scale).round() as u32;
    // 允许 1px 取整误差
    if saved.width + 1 < min_w || saved.height + 1 < min_h {
        return None;
    }
    let mut s = *saved;
    s.width = s.width.max(min_w).min(m.width.max(min_w));
    s.height = s.height.max(min_h).min(m.height.max(min_h));
    // 收缩后确保窗口完全落在该显示器内
    let max_x = m.x as i64 + m.width as i64 - s.width as i64;
    let max_y = m.y as i64 + m.height as i64 - s.height as i64;
    s.x = (s.x as i64).clamp(m.x as i64, max_x.max(m.x as i64)) as i32;
    s.y = (s.y as i64).clamp(m.y as i64, max_y.max(m.y as i64)) as i32;
    Some(s)
}

/// 防抖：记录最新快照与代数；只有最后一次变更后静默 `DEBOUNCE` 才写盘。
pub const DEBOUNCE_MS: u64 = 500;

#[derive(Default)]
pub struct Tracker {
    inner: std::sync::Mutex<(Option<WindowState>, u64, Option<WindowState>)>, // (current, generation, last_written)
}

impl Tracker {
    /// 更新快照（maximized 时仅更新标志，保留还原尺寸）。返回新代数。
    pub fn update(&self, maximized: bool, geom: Option<(i32, i32, u32, u32)>) -> u64 {
        let mut g = self.inner.lock().unwrap();
        let mut cur = g.0.unwrap_or_default();
        cur.maximized = maximized;
        if !maximized {
            if let Some((x, y, w, h)) = geom {
                if w > 0 && h > 0 {
                    cur.x = x;
                    cur.y = y;
                    cur.width = w;
                    cur.height = h;
                }
            }
        }
        g.0 = Some(cur);
        g.1 += 1;
        g.1
    }

    pub fn generation(&self) -> u64 {
        self.inner.lock().unwrap().1
    }

    /// 若有未写入的变化则写盘。
    pub fn flush(&self, p: &Path) -> Result<bool, String> {
        let mut g = self.inner.lock().unwrap();
        match g.0 {
            Some(cur) if cur.width > 0 && cur.height > 0 && g.2 != Some(cur) => {
                save(p, &cur)?;
                g.2 = Some(cur);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// 启动恢复后的初始快照（不触发写盘）。
    pub fn seed(&self, s: WindowState) {
        let mut g = self.inner.lock().unwrap();
        g.0 = Some(s);
        g.2 = Some(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon() -> Vec<MonitorRect> {
        vec![
            MonitorRect { x: 0, y: 0, width: 1920, height: 1080, scale: 1.0 },
            MonitorRect { x: 1920, y: 0, width: 2560, height: 1440, scale: 1.5 },
        ]
    }

    #[test]
    fn valid_state_restored() {
        let s = WindowState { x: 100, y: 80, width: 1000, height: 600, maximized: false };
        assert_eq!(validate(&s, &mon(), (720.0, 450.0)), Some(s));
    }

    #[test]
    fn too_small_rejected_with_monitor_scale() {
        let s = WindowState { x: 100, y: 80, width: 700, height: 600, maximized: false };
        assert_eq!(validate(&s, &mon(), (720.0, 450.0)), None);
        // 第二块 1.5x 屏：最小 1080x675
        let s = WindowState { x: 2000, y: 80, width: 1000, height: 700, maximized: false };
        assert_eq!(validate(&s, &mon(), (720.0, 450.0)), None);
        let s = WindowState { x: 2000, y: 80, width: 1080, height: 675, maximized: true };
        assert_eq!(validate(&s, &mon(), (720.0, 450.0)), Some(s));
    }

    #[test]
    fn offscreen_rejected() {
        let s = WindowState { x: -5000, y: 80, width: 1000, height: 600, maximized: false };
        assert_eq!(validate(&s, &mon(), (720.0, 450.0)), None);
        let s = WindowState { x: 100, y: 3000, width: 1000, height: 600, maximized: false };
        assert_eq!(validate(&s, &mon(), (720.0, 450.0)), None);
        assert_eq!(validate(&WindowState::default(), &mon(), (720.0, 450.0)), None);
        let s = WindowState { x: 100, y: 80, width: 1000, height: 600, maximized: false };
        assert_eq!(validate(&s, &[], (720.0, 450.0)), None);
    }

    #[test]
    fn oversize_clamped_into_monitor() {
        let s = WindowState { x: 50, y: 50, width: 3000, height: 2000, maximized: false };
        let v = validate(&s, &mon(), (720.0, 450.0)).unwrap();
        assert_eq!((v.x, v.y, v.width, v.height), (0, 0, 1920, 1080));
        // 右下溢出：拉回屏内
        let s = WindowState { x: 1500, y: 900, width: 800, height: 500, maximized: false };
        let v = validate(&s, &mon(), (720.0, 450.0)).unwrap();
        assert_eq!((v.x, v.y), (1120, 580));
    }

    #[test]
    fn tracker_keeps_restore_size_when_maximized_and_debounces() {
        let dir = std::env::temp_dir().join(format!("mcl-ws-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("window-state.json");
        let t = Tracker::default();
        t.seed(WindowState { x: 10, y: 10, width: 800, height: 500, maximized: false });
        assert!(!t.flush(&p).unwrap(), "未变化不写盘");
        let g1 = t.update(false, Some((20, 30, 900, 560)));
        let g2 = t.update(true, Some((0, 0, 1920, 1040)));
        assert!(g2 > g1 && t.generation() == g2);
        assert!(t.flush(&p).unwrap());
        let s = load(&p).unwrap();
        assert_eq!(s, WindowState { x: 20, y: 30, width: 900, height: 560, maximized: true });
        // 最小化（0 尺寸）不覆盖
        t.update(false, Some((-32000, -32000, 0, 0)));
        t.flush(&p).unwrap();
        let s = load(&p).unwrap();
        assert_eq!((s.width, s.height), (900, 560));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_ignored() {
        let dir = std::env::temp_dir().join(format!("mcl-ws2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("window-state.json");
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(load(&p), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
