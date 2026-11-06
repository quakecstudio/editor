// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings id's server and host register (PHYS-12): created with id's defaults when a
//! world loads, so the progs read and set them with `cvar` and `cvar_set` as they do in the
//! engine, and read by the physics where id's code reads `<setting>.value`.

/// Every setting `SV_Init` and `Host_InitLocal` register that the server reads, and the
/// default each is declared with (`WinQuake/sv_main.c`, `sv_phys.c`, `sv_user.c`,
/// `pr_cmds.c`, `host.c`, `net_main.c`, `cl_main.c`); `skill`, `deathmatch`, `coop` and
/// `teamplay` the engine already has.
pub(crate) const SERVER_SETTINGS: [(&str, &str); 24] = [
    ("hostname", "UNNAMED"),
    ("_cl_name", "player"),
    ("_cl_color", "0"),
    ("sv_maxvelocity", "2000"),
    ("sv_gravity", "800"),
    ("sv_friction", "4"),
    ("edgefriction", "2"),
    ("sv_stopspeed", "100"),
    ("sv_maxspeed", "320"),
    ("sv_accelerate", "10"),
    ("sv_idealpitchscale", "0.8"),
    ("sv_aim", "0.93"),
    ("sv_nostep", "0"),
    ("host_framerate", "0"),
    ("host_speeds", "0"),
    ("sys_ticrate", "0.05"),
    ("serverprofile", "0"),
    ("fraglimit", "0"),
    ("timelimit", "0"),
    ("samelevel", "0"),
    ("noexit", "0"),
    ("developer", "0"),
    ("pausable", "1"),
    ("temp1", "0"),
];

/// A setting the server's own code reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Setting {
    /// `sv_maxvelocity`, `SV_CheckVelocity`'s bound (PHYS-6.3).
    MaxVelocity,
    /// `sv_gravity` (PHYS-6.4).
    Gravity,
    /// `sv_friction` (PHYS-9.3, PHYS-9.5).
    Friction,
    /// `edgefriction`, the friction's factor at an edge (PHYS-9.3).
    EdgeFriction,
    /// `sv_stopspeed` (PHYS-9.3).
    StopSpeed,
    /// `sv_maxspeed` (PHYS-9.2, PHYS-9.5).
    MaxSpeed,
    /// `sv_accelerate` (PHYS-9.4, PHYS-9.5).
    Accelerate,
    /// `sv_idealpitchscale` (NET-4.2).
    IdealPitchScale,
    /// `sv_aim`, `aim`'s cone (WORLD-8.8).
    Aim,
    /// `sv_nostep`, which stops stepping up stairs (PHYS-8.3).
    NoStep,
    /// `teamplay`, which `aim` reads (WORLD-8.8).
    Teamplay,
    /// `pausable`, which lets a client pause (PHYS-11.8).
    Pausable,
}

impl Setting {
    /// The setting's name.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::MaxVelocity => "sv_maxvelocity",
            Self::Gravity => "sv_gravity",
            Self::Friction => "sv_friction",
            Self::EdgeFriction => "edgefriction",
            Self::StopSpeed => "sv_stopspeed",
            Self::MaxSpeed => "sv_maxspeed",
            Self::Accelerate => "sv_accelerate",
            Self::IdealPitchScale => "sv_idealpitchscale",
            Self::Aim => "sv_aim",
            Self::NoStep => "sv_nostep",
            Self::Teamplay => "teamplay",
            Self::Pausable => "pausable",
        }
    }
}

#[cfg(test)]
mod tests;
