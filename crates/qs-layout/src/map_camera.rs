// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's camera settings (P7-08, MAP-30): the 3D view's field of view and how fast
//! and which way the mouse and keys move the camera, as TrenchBroom's View and Mouse
//! preferences set them (its `Preferences.h`: `CameraFov`, `CameraLookSpeed`,
//! `CameraLookInvertV`, `CameraPanSpeed`, `CameraMouseWheelInvert`, `CameraFlyMoveSpeed`). A
//! command sets them, so the panel, the CLI and the agent share them, and the layout file keeps
//! them.

use core::fmt;

use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::HasLayout;

/// The id of the command that sets the map editor's camera settings.
pub const MAP_CAMERA: &str = "view.map_camera";

/// The 3D view's field of view across, in degrees.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "f32", into = "f32")]
pub struct MapFov(f32);

impl MapFov {
    /// The narrowest, 50 degrees: TrenchBroom's FOV slider's least (`ViewPreferencePane`,
    /// `SliderWithLabel{50, 150}`).
    pub const MIN: f32 = 50.0;
    /// The widest, 150 degrees: that slider's most.
    pub const MAX: f32 = 150.0;
    /// 90 degrees, Quake's `fov 90` and TrenchBroom's `CameraFov` default.
    pub const DEFAULT: Self = Self(90.0);

    /// The degrees.
    #[must_use]
    pub const fn degrees(self) -> f32 {
        self.0
    }
}

impl Default for MapFov {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TryFrom<f32> for MapFov {
    type Error = MapCameraError;

    fn try_from(degrees: f32) -> Result<Self, MapCameraError> {
        if (Self::MIN..=Self::MAX).contains(&degrees) {
            Ok(Self(degrees))
        } else {
            Err(MapCameraError::Fov(degrees))
        }
    }
}

impl From<MapFov> for f32 {
    fn from(fov: MapFov) -> Self {
        fov.0
    }
}

/// How fast the camera moves, as a factor of the editor's own speed: 1 is that speed.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "f32", into = "f32")]
pub struct CameraSpeed(f32);

impl CameraSpeed {
    /// The slowest, a tenth: TrenchBroom's `MinCameraFlyMoveSpeed`, 0.1.
    pub const MIN: f32 = 0.1;
    /// The fastest, ten times: TrenchBroom's `MaxCameraFlyMoveSpeed`, 10.
    pub const MAX: f32 = 10.0;
    /// The editor's own speed.
    pub const DEFAULT: Self = Self(1.0);

    /// The factor.
    #[must_use]
    pub const fn factor(self) -> f32 {
        self.0
    }
}

impl Default for CameraSpeed {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TryFrom<f32> for CameraSpeed {
    type Error = MapCameraError;

    fn try_from(factor: f32) -> Result<Self, MapCameraError> {
        if (Self::MIN..=Self::MAX).contains(&factor) {
            Ok(Self(factor))
        } else {
            Err(MapCameraError::Speed(factor))
        }
    }
}

impl From<CameraSpeed> for f32 {
    fn from(speed: CameraSpeed) -> Self {
        speed.0
    }
}

/// Which way a mouse movement moves the camera.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// The ordinary way: up looks up, the wheel away travels forward.
    #[default]
    Normal,
    /// The other way, as TrenchBroom's Invert settings.
    Inverted,
}

impl Direction {
    /// 1 the ordinary way, -1 inverted: what a movement is multiplied by.
    #[must_use]
    pub const fn sign(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Inverted => -1.0,
        }
    }
}

/// The map editor's camera settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MapCamera {
    /// The 3D view's field of view.
    pub fov: MapFov,
    /// How fast a right-drag turns the camera.
    pub look_speed: CameraSpeed,
    /// How fast a middle-drag pans it.
    pub pan_speed: CameraSpeed,
    /// How fast the keys fly it.
    pub fly_speed: CameraSpeed,
    /// Which way a vertical right-drag tilts it.
    pub look_vertical: Direction,
    /// Which way the wheel travels it.
    pub wheel: Direction,
}

/// A camera setting out of its range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MapCameraError {
    /// A field of view outside 50 to 150 degrees.
    Fov(f32),
    /// A speed outside 0.1 to 10.
    Speed(f32),
}

impl fmt::Display for MapCameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fov(degrees) => write!(
                f,
                "a field of view of {degrees} degrees is outside {} to {}",
                MapFov::MIN,
                MapFov::MAX
            ),
            Self::Speed(factor) => write!(
                f,
                "a camera speed of {factor} is outside {} to {}",
                CameraSpeed::MIN,
                CameraSpeed::MAX
            ),
        }
    }
}

impl std::error::Error for MapCameraError {}

/// The parameters of `view.map_camera`: the settings to change, the rest kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapCameraParams {
    /// The 3D view's field of view, 50 to 150 degrees.
    #[serde(default)]
    pub fov: Option<MapFov>,
    /// How fast a right-drag turns the camera, 0.1 to 10 times the editor's speed.
    #[serde(default)]
    pub look_speed: Option<CameraSpeed>,
    /// How fast a middle-drag pans it.
    #[serde(default)]
    pub pan_speed: Option<CameraSpeed>,
    /// How fast the keys fly it.
    #[serde(default)]
    pub fly_speed: Option<CameraSpeed>,
    /// Which way a vertical right-drag tilts it.
    #[serde(default)]
    pub look_vertical: Option<Direction>,
    /// Which way the wheel travels it.
    #[serde(default)]
    pub wheel: Option<Direction>,
}

/// `view.map_camera`.
struct SetMapCamera;

impl<C: HasLayout + 'static> Command<C> for SetMapCamera {
    const ID: &'static str = MAP_CAMERA;
    const SUMMARY: &'static str = "Sets the map editor's camera: the 3D view's field of view, \
        50 to 150 degrees, how fast the mouse and keys move it, 0.1 to 10 times, and which way \
        a vertical look and the wheel go. Settings not given are kept.";
    type Params = MapCameraParams;
    type Output = MapCamera;
    type Error = !;

    fn run(&self, context: &mut C, params: MapCameraParams) -> Result<Outcome<MapCamera, C>, !> {
        let camera = &mut context.layout_mut().map_camera;
        let MapCameraParams {
            fov,
            look_speed,
            pan_speed,
            fly_speed,
            look_vertical,
            wheel,
        } = params;
        camera.fov = fov.unwrap_or(camera.fov);
        camera.look_speed = look_speed.unwrap_or(camera.look_speed);
        camera.pan_speed = pan_speed.unwrap_or(camera.pan_speed);
        camera.fly_speed = fly_speed.unwrap_or(camera.fly_speed);
        camera.look_vertical = look_vertical.unwrap_or(camera.look_vertical);
        camera.wheel = wheel.unwrap_or(camera.wheel);
        Ok(Outcome::outside_history(*camera))
    }
}

/// Registers `view.map_camera`.
pub(crate) fn register<C: HasLayout + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(SetMapCamera)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qs_commands::{Registry, Session};
    use serde_json::json;

    use super::{CameraSpeed, Direction, MAP_CAMERA, MapCamera, MapFov};
    use crate::Layout;

    // No spec statement: P7-08: view.map_camera changes the settings it is given and keeps the
    // rest, outside the undo history; the layout file keeps them, an older file reads as
    // TrenchBroom's defaults, and a setting out of its range is refused
    #[test]
    fn the_camera_settings_are_set_kept_and_held_to_their_ranges() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new(Layout::default(), registry);
        assert_eq!(session.context().map_camera, MapCamera::default());
        assert_eq!(MapCamera::default().fov.degrees(), 90.0);
        session
            .execute(
                MAP_CAMERA,
                json!({ "fov": 110, "look_vertical": "inverted" }),
            )
            .unwrap();
        session
            .execute(MAP_CAMERA, json!({ "fly_speed": 2.5 }))
            .unwrap();
        let camera = session.context().map_camera;
        assert_eq!(camera.fov.degrees(), 110.0);
        assert_eq!(camera.look_vertical, Direction::Inverted);
        assert_eq!(camera.fly_speed.factor(), 2.5);
        assert_eq!(camera.look_speed, CameraSpeed::DEFAULT);
        assert_eq!(camera.wheel, Direction::Normal);
        assert!(session.undo().is_err());
        let written = toml::to_string(&session.context().as_written()).unwrap();
        let read: Layout = toml::from_str(&written).unwrap();
        assert_eq!(read.map_camera, camera);
        let older: Layout = toml::from_str("[left]\nvisible = true\nsize = 240.0\n").unwrap();
        assert_eq!(older.map_camera, MapCamera::default());
        for bad in [
            json!({ "fov": 49.9 }),
            json!({ "fov": 150.1 }),
            json!({ "look_speed": 0.0 }),
            json!({ "pan_speed": 10.5 }),
            json!({ "wheel": "sideways" }),
            json!({ "zoom": 2 }),
        ] {
            assert!(session.execute(MAP_CAMERA, bad.clone()).is_err(), "{bad}");
        }
        for edge in [MapFov::MIN, MapFov::MAX] {
            session.execute(MAP_CAMERA, json!({ "fov": edge })).unwrap();
            assert_eq!(session.context().map_camera.fov.degrees(), edge);
        }
        assert_eq!(Direction::Normal.sign(), 1.0);
        assert_eq!(Direction::Inverted.sign(), -1.0);
    }
}
