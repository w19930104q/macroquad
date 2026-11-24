//! Regression test for the nested-camera viewport stack (fixed sample MQ-A).
//!
//! `pop_camera_state` used to restore the render pass, depth test and camera
//! matrix, but not the GL viewport. After leaving a split-screen camera whose
//! viewport was `(100, 50, 320, 240)`, the active viewport stayed clipped to
//! that rectangle, so subsequent draw calls rendered into the small region and
//! mouse picking / drag alignment offset towards the top-left corner.
//!
//! Sample MQ-A:
//!   outer viewport: (0, 0, 800, 600)
//!   inner viewport: (100, 50, 320, 240)
//!
//! All assertions happen inside a single frame on purpose. `experimental::scene`
//! pushes a full-screen default camera on every `next_frame`, which would mask
//! a missing viewport restore across frame boundaries.

use macroquad::prelude::*;
use macroquad::test_helpers::active_viewport;

/// A camera whose world units equal screen pixels across its viewport:
/// world (0, 0) is the viewport's top-left and the viewport center maps to
/// itself. For the outer viewport this makes world (400, 300) -> screen (400, 300).
fn pixel_perfect_camera(viewport: (i32, i32, i32, i32)) -> Camera2D {
    let (_, _, vw, vh) = viewport;
    Camera2D {
        target: vec2(vw as f32 / 2.0, vh as f32 / 2.0),
        zoom: vec2(2.0 / vw as f32, 2.0 / vh as f32),
        offset: vec2(0.0, 0.0),
        rotation: 0.0,
        render_target: None,
        viewport: Some(viewport),
    }
}

fn assert_vec2_eq(actual: Vec2, expected: Vec2, what: &str) {
    assert!(
        (actual.x - expected.x).abs() < 1e-4 && (actual.y - expected.y).abs() < 1e-4,
        "{what}: expected {expected:?}, got {actual:?}"
    );
}

#[macroquad::test]
async fn camera_stack_restores_viewport() {
    assert_eq!(screen_width(), 800.0);
    assert_eq!(screen_height(), 600.0);

    let outer = pixel_perfect_camera((0, 0, 800, 600));
    let inner = pixel_perfect_camera((100, 50, 320, 240));

    // ---- Phase 1: outer camera, viewport (0, 0, 800, 600) ----
    set_default_camera();
    set_camera(&outer);
    assert_eq!(
        active_viewport(),
        Some((0, 0, 800, 600)),
        "outer: active viewport must be the full screen"
    );
    assert_vec2_eq(
        outer.world_to_screen(vec2(400.0, 300.0)),
        vec2(400.0, 300.0),
        "outer world_to_screen(400, 300)",
    );
    assert_vec2_eq(
        outer.screen_to_world(vec2(400.0, 300.0)),
        vec2(400.0, 300.0),
        "outer screen_to_world(400, 300)",
    );

    // ---- Phase 2: enter the nested inner camera, viewport (100, 50, 320, 240) ----
    push_camera_state();
    set_camera(&inner);
    assert_eq!(
        active_viewport(),
        Some((100, 50, 320, 240)),
        "inner: active viewport must be the nested viewport"
    );
    // The inner camera covers world x[0,320], y[0,240]. Its corners land on
    // the nested viewport's screen rectangle (bottom-left GL origin accounted
    // for in world_to_screen):
    //   world (0, 0)     -> screen (100, 310)   (viewport bottom-left)
    //   world (320, 240) -> screen (420, 550)   (viewport top-right)
    assert_vec2_eq(
        inner.world_to_screen(vec2(0.0, 0.0)),
        vec2(100.0, 310.0),
        "inner world_to_screen(0, 0)",
    );
    assert_vec2_eq(
        inner.world_to_screen(vec2(320.0, 240.0)),
        vec2(420.0, 550.0),
        "inner world_to_screen(320, 240)",
    );
    let inner_mapping = inner.world_to_screen(vec2(400.0, 300.0));
    let inner_pick = inner.screen_to_world(vec2(400.0, 300.0));
    assert_vec2_eq(
        inner_mapping,
        vec2(500.0, 610.0),
        "inner world_to_screen(400, 300)",
    );
    assert_vec2_eq(
        inner_pick,
        vec2(300.0, -10.0),
        "inner screen_to_world(400, 300)",
    );

    // ---- Phase 3: pop back to the outer camera ----
    pop_camera_state();
    assert_eq!(
        active_viewport(),
        Some((0, 0, 800, 600)),
        "pop: active viewport must return to the outer full-screen viewport"
    );
    // This is the mouse-picking / drag alignment that used to drift top-left:
    // the restored outer camera must map world (400, 300) back to (400, 300).
    assert_vec2_eq(
        outer.world_to_screen(vec2(400.0, 300.0)),
        vec2(400.0, 300.0),
        "pop world_to_screen(400, 300)",
    );
    assert_vec2_eq(
        outer.screen_to_world(vec2(400.0, 300.0)),
        vec2(400.0, 300.0),
        "pop screen_to_world(400, 300)",
    );

    // ---- Phase 4: re-enter the same inner camera; results must reproduce ----
    push_camera_state();
    set_camera(&inner);
    assert_eq!(
        active_viewport(),
        Some((100, 50, 320, 240)),
        "re-enter: nested viewport must be applied again"
    );
    assert_vec2_eq(
        inner.world_to_screen(vec2(400.0, 300.0)),
        inner_mapping,
        "re-enter world_to_screen must reproduce",
    );
    assert_vec2_eq(
        inner.screen_to_world(vec2(400.0, 300.0)),
        inner_pick,
        "re-enter screen_to_world must reproduce",
    );
    pop_camera_state();
    assert_eq!(
        active_viewport(),
        Some((0, 0, 800, 600)),
        "re-enter pop: outer viewport must be restored again"
    );
    assert_vec2_eq(
        outer.world_to_screen(vec2(400.0, 300.0)),
        vec2(400.0, 300.0),
        "re-enter pop world_to_screen(400, 300)",
    );

    // ---- The legacy default camera (no explicit viewport) is unaffected ----
    set_default_camera();
    assert_eq!(
        active_viewport(),
        None,
        "default camera: viewport state must be None (whole screen)"
    );
    assert_eq!(Camera2D::default().viewport, None);
}
