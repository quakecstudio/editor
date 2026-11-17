// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's 3D view from the keyboard (UI-23, #971): commands that turn the camera to
//! a standard view, orbit, pan and zoom it by a step, and save and bring back up to four
//! views. Each takes no parameter, so each has a key and runs from the command line by its
//! name. A command asks; the window's map editor, which has the camera, carries the last ask
//! out once, as it does an editor action. The saved views are kept in the layout file.

use core::fmt;

use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::HasLayout;

/// The id of the command the window runs to keep the 3D view's camera as a saved view.
pub const MAP_KEEP_VIEW: &str = "view.map_keep_view";

/// How far one orbit command turns the camera, in degrees.
pub const ORBIT_STEP: f32 = 15.0;
/// How far one pan command moves the camera, in map units: two of the default grid's steps.
pub const PAN_STEP: f32 = 32.0;
/// How far one zoom command moves the camera along its view, in map units.
pub const ZOOM_STEP: f32 = 64.0;

/// A view along one of the map's axes, or from a corner above, with the whole map in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StandardView {
    /// From above, looking down, north (+y) up.
    Top,
    /// From below, looking up, north (+y) up.
    Bottom,
    /// From the south, looking north (+y).
    Front,
    /// From the north, looking south (-y).
    Back,
    /// From the west, looking east (+x).
    Left,
    /// From the east, looking west (-x).
    Right,
    /// From the south-west and above, looking north-east and down.
    Isometric,
}

/// The yaw that looks east (+x), and those a quarter turn on each: north (+y), west and south.
const EAST: f32 = 0.0;
const NORTH: f32 = 90.0;
const WEST: f32 = 180.0;
const SOUTH: f32 = 270.0;
/// The yaw that looks north-east, between east and north.
const NORTH_EAST: f32 = 45.0;
/// The steepest the camera looks up or down, in degrees: the 3D view's own limit.
const STEEPEST: f32 = 89.0;
/// The pitch of a view down a cube's diagonal, in degrees: `atan(1 / sqrt(2))`.
const ISOMETRIC_PITCH: f32 = 35.264_39;

impl StandardView {
    /// Every standard view.
    pub const ALL: [Self; 7] = [
        Self::Top,
        Self::Bottom,
        Self::Front,
        Self::Back,
        Self::Left,
        Self::Right,
        Self::Isometric,
    ];

    /// The camera's yaw and pitch in the view, in degrees: yaw 0 looks down +x and grows
    /// towards +y, and a positive pitch looks down.
    #[must_use]
    pub const fn heading(self) -> (f32, f32) {
        match self {
            Self::Top => (NORTH, STEEPEST),
            Self::Bottom => (NORTH, -STEEPEST),
            Self::Front => (NORTH, 0.0),
            Self::Back => (SOUTH, 0.0),
            Self::Left => (EAST, 0.0),
            Self::Right => (WEST, 0.0),
            Self::Isometric => (NORTH_EAST, ISOMETRIC_PITCH),
        }
    }
}

/// One of the four places a view is saved in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ViewSlot {
    /// The first.
    #[serde(rename = "1")]
    One,
    /// The second.
    #[serde(rename = "2")]
    Two,
    /// The third.
    #[serde(rename = "3")]
    Three,
    /// The fourth.
    #[serde(rename = "4")]
    Four,
}

impl fmt::Display for ViewSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::One => "1",
            Self::Two => "2",
            Self::Three => "3",
            Self::Four => "4",
        })
    }
}

/// Where the 3D view's camera is and which way it looks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapPose {
    /// Its place, in map units.
    pub origin: [f32; 3],
    /// Its yaw in degrees: 0 looks down +x, growing towards +y.
    pub yaw: f32,
    /// Its pitch in degrees, positive looking down.
    pub pitch: f32,
}

/// The saved views, as the layout file keeps them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MapViews {
    #[serde(skip_serializing_if = "Option::is_none")]
    one: Option<MapPose>,
    #[serde(skip_serializing_if = "Option::is_none")]
    two: Option<MapPose>,
    #[serde(skip_serializing_if = "Option::is_none")]
    three: Option<MapPose>,
    #[serde(skip_serializing_if = "Option::is_none")]
    four: Option<MapPose>,
}

impl MapViews {
    /// The view saved in `slot`, if one is.
    #[must_use]
    pub const fn get(&self, slot: ViewSlot) -> Option<MapPose> {
        match slot {
            ViewSlot::One => self.one,
            ViewSlot::Two => self.two,
            ViewSlot::Three => self.three,
            ViewSlot::Four => self.four,
        }
    }

    /// Saves `pose` in `slot`, over the view saved there.
    const fn keep(&mut self, slot: ViewSlot, pose: MapPose) {
        let place = match slot {
            ViewSlot::One => &mut self.one,
            ViewSlot::Two => &mut self.two,
            ViewSlot::Three => &mut self.three,
            ViewSlot::Four => &mut self.four,
        };
        *place = Some(pose);
    }
}

/// What a command asked of the 3D view's camera, carried out by the map editor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MapLook {
    /// Turn to a standard view, with the whole map in it.
    Standard(StandardView),
    /// Turn by these degrees of yaw and pitch about the selection, or the point ahead.
    Orbit {
        /// Degrees to the left.
        yaw: f32,
        /// Degrees down.
        pitch: f32,
    },
    /// Move by these map units across and up, keeping the heading.
    Pan {
        /// Units to the camera's right.
        right: f32,
        /// Units up.
        up: f32,
    },
    /// Move this many map units along the view: forwards zooms in.
    Zoom(f32),
    /// Save where the camera is in this slot: the editor answers with `view.map_keep_view`.
    Save(ViewSlot),
    /// Go to this place and heading: a saved view.
    Pose(MapPose),
}

/// Why a view command was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapLookError {
    /// No view is saved in the slot.
    Empty(ViewSlot),
}

impl fmt::Display for MapLookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty(slot) => write!(f, "no view is saved in {slot}"),
        }
    }
}

impl core::error::Error for MapLookError {}

/// What a view command does with the layout.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Asked {
    /// Ask the editor for this.
    Look(MapLook),
    /// Ask the editor to go to the view saved in this slot.
    Recall(ViewSlot),
}

/// A command that asks the map editor to move the 3D view's camera and takes no parameters.
trait Look: Send + Sync + 'static {
    const ID: &'static str;
    const SUMMARY: &'static str;
    const ASKED: Asked;
}

/// The command of a [`Look`].
struct Looking<L>(core::marker::PhantomData<L>);

impl<L: Look, C: HasLayout + 'static> Command<C> for Looking<L> {
    const ID: &'static str = L::ID;
    const SUMMARY: &'static str = L::SUMMARY;
    type Params = NoParams;
    type Output = ();
    type Error = MapLookError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<(), C>, MapLookError> {
        let layout = context.layout_mut();
        let look = match L::ASKED {
            Asked::Look(look) => look,
            Asked::Recall(slot) => layout
                .map_views
                .get(slot)
                .map(MapLook::Pose)
                .ok_or(MapLookError::Empty(slot))?,
        };
        layout.map_look = Some(look);
        layout.map_looks = layout.map_looks.wrapping_add(1);
        Ok(Outcome::outside_history(()))
    }
}

/// Declares the [`Look`] commands and [`MAP_LOOKS`], the ids of them all.
macro_rules! looking {
    ($count:literal; $(($name:ident, $id:literal, $asked:expr, $summary:literal)),+ $(,)?) => {
        $(
            #[doc = concat!("`", $id, "`.")]
            struct $name;

            impl Look for $name {
                const ID: &'static str = $id;
                const SUMMARY: &'static str = $summary;
                const ASKED: Asked = $asked;
            }
        )+

        /// The id of every command that moves the 3D view's camera, and what it asks for; a
        /// saved view's command is listed with its slot.
        pub const MAP_LOOKS: [(&str, Option<MapLook>); $count] = [
            $(($id, match $asked {
                Asked::Look(look) => Some(look),
                Asked::Recall(_) => None,
            })),+
        ];

        /// Registers the view commands.
        fn register_looks<C: HasLayout + 'static>(
            registry: &mut Registry<C>,
        ) -> Result<(), RegisterError> {
            $(registry.register(Looking::<$name>(core::marker::PhantomData))?;)+
            Ok(())
        }
    };
}

looking!(25;
    (ViewTop, "view.map_top", Asked::Look(MapLook::Standard(StandardView::Top)),
        "Turns the map editor's 3D view to look down on the whole map from above, north up."),
    (ViewBottom, "view.map_bottom", Asked::Look(MapLook::Standard(StandardView::Bottom)),
        "Turns the map editor's 3D view to look up at the whole map from below, north up."),
    (ViewFront, "view.map_front", Asked::Look(MapLook::Standard(StandardView::Front)),
        "Turns the map editor's 3D view to look north at the whole map, from the south."),
    (ViewBack, "view.map_back", Asked::Look(MapLook::Standard(StandardView::Back)),
        "Turns the map editor's 3D view to look south at the whole map, from the north."),
    (ViewLeft, "view.map_left", Asked::Look(MapLook::Standard(StandardView::Left)),
        "Turns the map editor's 3D view to look east at the whole map, from the west."),
    (ViewRight, "view.map_right", Asked::Look(MapLook::Standard(StandardView::Right)),
        "Turns the map editor's 3D view to look west at the whole map, from the east."),
    (ViewIsometric, "view.map_isometric",
        Asked::Look(MapLook::Standard(StandardView::Isometric)),
        "Turns the map editor's 3D view to look at the whole map from the south-west and above."),
    (OrbitLeft, "view.map_orbit_left",
        Asked::Look(MapLook::Orbit { yaw: ORBIT_STEP, pitch: 0.0 }),
        "Turns the map editor's 3D view 15 degrees to the left about the selection, or about \
         the point ahead when nothing is selected."),
    (OrbitRight, "view.map_orbit_right",
        Asked::Look(MapLook::Orbit { yaw: -ORBIT_STEP, pitch: 0.0 }),
        "Turns the map editor's 3D view 15 degrees to the right about the selection, or about \
         the point ahead when nothing is selected."),
    (OrbitUp, "view.map_orbit_up",
        Asked::Look(MapLook::Orbit { yaw: 0.0, pitch: -ORBIT_STEP }),
        "Turns the map editor's 3D view 15 degrees up about the selection, or about the point \
         ahead when nothing is selected."),
    (OrbitDown, "view.map_orbit_down",
        Asked::Look(MapLook::Orbit { yaw: 0.0, pitch: ORBIT_STEP }),
        "Turns the map editor's 3D view 15 degrees down about the selection, or about the \
         point ahead when nothing is selected."),
    (PanLeft, "view.map_pan_left", Asked::Look(MapLook::Pan { right: -PAN_STEP, up: 0.0 }),
        "Moves the map editor's 3D view 32 units to its left, keeping its heading."),
    (PanRight, "view.map_pan_right", Asked::Look(MapLook::Pan { right: PAN_STEP, up: 0.0 }),
        "Moves the map editor's 3D view 32 units to its right, keeping its heading."),
    (PanUp, "view.map_pan_up", Asked::Look(MapLook::Pan { right: 0.0, up: PAN_STEP }),
        "Moves the map editor's 3D view 32 units up, keeping its heading."),
    (PanDown, "view.map_pan_down", Asked::Look(MapLook::Pan { right: 0.0, up: -PAN_STEP }),
        "Moves the map editor's 3D view 32 units down, keeping its heading."),
    (ZoomIn, "view.map_zoom_in", Asked::Look(MapLook::Zoom(ZOOM_STEP)),
        "Moves the map editor's 3D view 64 units forwards along its view."),
    (ZoomOut, "view.map_zoom_out", Asked::Look(MapLook::Zoom(-ZOOM_STEP)),
        "Moves the map editor's 3D view 64 units back along its view."),
    (SaveView1, "view.map_save_view_1", Asked::Look(MapLook::Save(ViewSlot::One)),
        "Saves where the map editor's 3D view is as view 1, kept in the layout file."),
    (SaveView2, "view.map_save_view_2", Asked::Look(MapLook::Save(ViewSlot::Two)),
        "Saves where the map editor's 3D view is as view 2, kept in the layout file."),
    (SaveView3, "view.map_save_view_3", Asked::Look(MapLook::Save(ViewSlot::Three)),
        "Saves where the map editor's 3D view is as view 3, kept in the layout file."),
    (SaveView4, "view.map_save_view_4", Asked::Look(MapLook::Save(ViewSlot::Four)),
        "Saves where the map editor's 3D view is as view 4, kept in the layout file."),
    (View1, "view.map_view_1", Asked::Recall(ViewSlot::One),
        "Brings the map editor's 3D view back to saved view 1."),
    (View2, "view.map_view_2", Asked::Recall(ViewSlot::Two),
        "Brings the map editor's 3D view back to saved view 2."),
    (View3, "view.map_view_3", Asked::Recall(ViewSlot::Three),
        "Brings the map editor's 3D view back to saved view 3."),
    (View4, "view.map_view_4", Asked::Recall(ViewSlot::Four),
        "Brings the map editor's 3D view back to saved view 4."),
);

/// The parameters of `view.map_keep_view`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeepViewParams {
    /// The slot to save the view in.
    pub slot: ViewSlot,
    /// The camera's place, in map units.
    pub origin: [f32; 3],
    /// Its yaw in degrees: 0 looks down +x, growing towards +y.
    pub yaw: f32,
    /// Its pitch in degrees, positive looking down.
    pub pitch: f32,
}

/// `view.map_keep_view`.
struct KeepView;

impl<C: HasLayout + 'static> Command<C> for KeepView {
    const ID: &'static str = MAP_KEEP_VIEW;
    const SUMMARY: &'static str = "Keeps a place and heading of the map editor's 3D view as a \
        saved view. The window runs it for `view.map_save_view_1` to `_4`, with its camera.";
    type Params = KeepViewParams;
    type Output = ();
    type Error = MapLookError;

    fn run(&self, context: &mut C, params: KeepViewParams) -> Result<Outcome<(), C>, MapLookError> {
        let KeepViewParams {
            slot,
            origin,
            yaw,
            pitch,
        } = params;
        let pose = MapPose { origin, yaw, pitch };
        context.layout_mut().map_views.keep(slot, pose);
        Ok(Outcome::outside_history(()))
    }
}

/// Registers the view commands and `view.map_keep_view`.
pub(crate) fn register<C: HasLayout + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    register_looks(registry)?;
    registry.register(KeepView)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qs_commands::{Registry, Session};
    use serde_json::json;

    use super::{MAP_KEEP_VIEW, MAP_LOOKS, MapLook, MapPose, StandardView, ViewSlot};
    use crate::Layout;

    fn session() -> Session<Layout> {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        Session::new(Layout::default(), registry)
    }

    // No spec statement: UI-23, #971: each view command asks the editor for its one motion,
    // counted so the editor carries each out once, outside the undo history
    #[test]
    fn each_view_command_asks_for_its_motion_once() {
        let mut session = session();
        let mut asked = 0;
        for (id, look) in MAP_LOOKS {
            let Some(look) = look else { continue };
            session.execute(id, json!({})).unwrap();
            asked += 1;
            let layout = session.context();
            assert_eq!(
                (layout.map_look, layout.map_looks),
                (Some(look), asked),
                "{id}"
            );
        }
        assert_eq!(asked, 21);
        assert!(session.undo().is_err());
        assert!(session.execute("view.map_top", json!({ "by": 1 })).is_err());
    }

    // No spec statement: UI-23, #971: the standard views look along the map's axes, north up
    // from above and below, and the isometric view looks north-east and down a cube's diagonal
    #[test]
    fn the_standard_views_look_along_the_axes() {
        let headings: Vec<(f32, f32)> = StandardView::ALL.iter().map(|v| v.heading()).collect();
        assert_eq!(
            headings,
            [
                (90.0, 89.0),
                (90.0, -89.0),
                (90.0, 0.0),
                (270.0, 0.0),
                (0.0, 0.0),
                (180.0, 0.0),
                (45.0, 35.264_39),
            ]
        );
        let diagonal = (1.0_f32 / 2.0_f32.sqrt()).atan().to_degrees();
        assert!((diagonal - 35.264_39).abs() < 1e-4, "{diagonal}");
    }

    // No spec statement: UI-23, #971: a saved view is asked for by its slot's command, kept by
    // view.map_keep_view, written to the layout file and brought back as the pose kept; an
    // empty slot's command is refused and asks for nothing
    #[test]
    fn a_view_is_saved_kept_in_the_file_and_brought_back() {
        let mut session = session();
        let refused = session.execute("view.map_view_2", json!({})).unwrap_err();
        assert!(
            refused.to_string().contains("no view is saved in 2"),
            "{refused}"
        );
        assert_eq!(session.context().map_looks, 0);
        session.execute("view.map_save_view_2", json!({})).unwrap();
        assert_eq!(
            session.context().map_look,
            Some(MapLook::Save(ViewSlot::Two))
        );
        session
            .execute(
                MAP_KEEP_VIEW,
                json!({ "slot": "2", "origin": [8, -16, 24], "yaw": 45, "pitch": 10 }),
            )
            .unwrap();
        let pose = MapPose {
            origin: [8.0, -16.0, 24.0],
            yaw: 45.0,
            pitch: 10.0,
        };
        assert_eq!(session.context().map_views.get(ViewSlot::Two), Some(pose));
        assert_eq!(session.context().map_views.get(ViewSlot::One), None);
        session.execute("view.map_view_2", json!({})).unwrap();
        assert_eq!(session.context().map_look, Some(MapLook::Pose(pose)));
        let written = toml::to_string(&session.context().as_written()).unwrap();
        let read: Layout = toml::from_str(&written).unwrap();
        assert_eq!(read.map_views, session.context().map_views);
        // The ask itself is not written.
        assert_eq!((read.map_look, read.map_looks), (None, 0));
        let older: Layout = toml::from_str("[left]\nvisible = true\nsize = 240.0\n").unwrap();
        assert_eq!(older.map_views.get(ViewSlot::Two), None);
        for bad in [
            json!({ "slot": "5", "origin": [0, 0, 0], "yaw": 0, "pitch": 0 }),
            json!({ "slot": "1", "origin": [0, 0], "yaw": 0, "pitch": 0 }),
            json!({ "slot": "1" }),
        ] {
            assert!(
                session.execute(MAP_KEEP_VIEW, bad.clone()).is_err(),
                "{bad}"
            );
        }
    }
}
