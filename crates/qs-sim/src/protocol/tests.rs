// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    EntAlpha, EntScale, Protocol, ProtocolFlags, ProtocolVersion, WireAngle, WireCoord, q_rint,
};

/// RMQ's 999 with `flags`.
fn rmq(flags: ProtocolFlags) -> Protocol {
    Protocol::new(ProtocolVersion::Rmq, flags)
}

// spec NET-6.1: 666 by default; 999 with PRFL_INT32COORD | PRFL_SHORTANGLE; the numbers 15,
// 666 and 999 and no others; flags only for 999
#[test]
fn the_protocols_and_their_numbers() {
    assert_eq!(Protocol::default(), Protocol::FITZQUAKE);
    assert_eq!(Protocol::RMQ.flags().bits(), (1 << 7) | (1 << 1));
    for (version, number) in [
        (ProtocolVersion::NetQuake, 15),
        (ProtocolVersion::FitzQuake, 666),
        (ProtocolVersion::Rmq, 999),
    ] {
        assert_eq!(version.number(), number);
        assert_eq!(ProtocolVersion::from_number(number), Some(version));
    }
    assert_eq!(ProtocolVersion::from_number(16), None);
    assert_eq!(
        Protocol::new(ProtocolVersion::FitzQuake, ProtocolFlags::FLOAT_COORD).flags(),
        ProtocolFlags::NONE
    );
    assert!(Protocol::NETQUAKE.is_netquake());
    assert!(!Protocol::FITZQUAKE.is_netquake());
}

// spec NET-6.2: 15 truncates as id does; 666 rounds with Q_rint; 999's flags pick a long of
// sixteenths, a float or the 24-bit form
#[test]
fn coordinates_by_protocol() {
    assert_eq!(Protocol::NETQUAKE.coord(-0.9), WireCoord::Short(-7));
    assert_eq!(Protocol::NETQUAKE.coord(1.99), WireCoord::Short(15));
    assert_eq!(Protocol::FITZQUAKE.coord(-0.9), WireCoord::Short(-7));
    assert_eq!(Protocol::FITZQUAKE.coord(1.99), WireCoord::Short(16));
    assert_eq!(Protocol::FITZQUAKE.coord(-1.99), WireCoord::Short(-16));
    assert_eq!(Protocol::RMQ.coord(1.99), WireCoord::Long(32));
    assert_eq!(
        rmq(ProtocolFlags::FLOAT_COORD.with(ProtocolFlags::INT32_COORD)).coord(1.25),
        WireCoord::Float(1.25),
        "the float wins"
    );
    // 2.5 * 255 = 637.5, (int) 637, % 255 = 127.
    assert_eq!(
        rmq(ProtocolFlags::COORD_24).coord(2.5),
        WireCoord::Short24(2, 127)
    );
    assert_eq!(
        rmq(ProtocolFlags::COORD_24.with(ProtocolFlags::INT32_COORD)).coord(2.5),
        WireCoord::Long(40),
        "the long wins over the 24-bit form"
    );
}

// spec NET-6.2: 15's angle byte is id's ((int)f*256/360)&255; 666's Q_rint(f*256/360)&255;
// 999's short Q_rint(f*65536/360)&65535; a float with PRFL_FLOATANGLE; MSG_WriteAngle16 is
// the short unless the float
#[test]
fn angles_by_protocol() {
    assert_eq!(Protocol::NETQUAKE.angle(359.9), WireAngle::Byte(255));
    assert_eq!(Protocol::NETQUAKE.angle(-90.0), WireAngle::Byte(192));
    assert_eq!(
        Protocol::FITZQUAKE.angle(359.9),
        WireAngle::Byte(0),
        "rounded to 256"
    );
    assert_eq!(Protocol::FITZQUAKE.angle(1.0), WireAngle::Byte(1));
    assert_eq!(Protocol::FITZQUAKE.angle(-90.0), WireAngle::Byte(192));
    assert_eq!(Protocol::RMQ.angle(90.0), WireAngle::Short(16384));
    assert_eq!(Protocol::RMQ.angle(-1.0), WireAngle::Short(65354));
    assert_eq!(
        rmq(ProtocolFlags::FLOAT_ANGLE).angle(12.5),
        WireAngle::Float(12.5)
    );
    assert_eq!(Protocol::FITZQUAKE.angle16(90.0), WireAngle::Short(16384));
    assert_eq!(
        rmq(ProtocolFlags::FLOAT_ANGLE).angle16(7.0),
        WireAngle::Float(7.0)
    );
}

// spec NET-6.2: Q_rint rounds half away from zero, and 0 to 0
#[test]
fn q_rint_rounds_half_away_from_zero() {
    assert_eq!(q_rint(0.5), 1);
    assert_eq!(q_rint(-0.5), -1);
    assert_eq!(q_rint(0.49), 0);
    assert_eq!(q_rint(0.0), 0);
    assert_eq!(q_rint(-2.6), -3);
}

// spec NET-6.8: ENTALPHA_ENCODE is 0 for 0, else Q_rint of a*254+1 clamped to 1..255;
// ENTALPHA_DECODE is 1 for the default, else (a-1)/254
#[test]
fn alpha_encodes_as_quakespasm_does() {
    assert_eq!(EntAlpha::encode(0.0), EntAlpha::DEFAULT);
    assert_eq!(EntAlpha::encode(1.0).byte(), 255);
    assert_eq!(EntAlpha::encode(0.5).byte(), 128);
    assert_eq!(EntAlpha::encode(-3.0).byte(), 1, "clamped to ENTALPHA_ZERO");
    assert!(EntAlpha::encode(-3.0).is_zero());
    assert_eq!(EntAlpha::encode(9.0).byte(), 255, "clamped to ENTALPHA_ONE");
    assert_eq!(EntAlpha::DEFAULT.decode(), 1.0);
    assert_eq!(EntAlpha::from_byte(1).decode(), 0.0);
    assert_eq!(EntAlpha::from_byte(255).decode(), 1.0);
    assert_eq!(EntAlpha::from_byte(128).decode(), 0.5);
}

// spec NET-6.8: ENTSCALE_ENCODE is 16 for 0, else a*16 kept to its low byte; decode is a/16
#[test]
fn scale_encodes_as_quakespasm_does() {
    assert_eq!(EntScale::encode(0.0), EntScale::DEFAULT);
    assert_eq!(EntScale::encode(1.0).byte(), 16);
    assert_eq!(EntScale::encode(2.5).byte(), 40);
    assert_eq!(EntScale::encode(16.5).byte(), 8, "264's low byte");
    assert_eq!(EntScale::default(), EntScale::DEFAULT);
    assert_eq!(EntScale::from_byte(40).decode(), 2.5);
    assert_eq!(EntScale::DEFAULT.decode(), 1.0);
}

// spec NET-6.2: an angle read back as the client reads it: 15's and 666's signed byte times
// 360/256, 999's signed short times 360/65536
#[test]
fn angles_read_back() {
    assert_eq!(Protocol::NETQUAKE.wire_angle(100.0), 99.843_75);
    assert_eq!(Protocol::NETQUAKE.wire_angle(270.0), -90.0);
    assert_eq!(Protocol::FITZQUAKE.wire_angle(100.0), 99.843_75);
    assert_eq!(Protocol::RMQ.wire_angle(100.0), 99.997_56);
    assert_eq!(Protocol::RMQ.wire_angle(270.0), -90.0);
    assert_eq!(rmq(ProtocolFlags::FLOAT_ANGLE).wire_angle(12.25), 12.25);
}

// spec NET-6.2: Q_rint adds C's double 0.5, so a float just under a half rounds down, where a
// float sum would round to 1
#[test]
fn q_rint_adds_a_double_half() {
    let just_under = 0.499_999_97_f32;
    assert_eq!(just_under + 0.5, 1.0, "the float sum rounds up");
    assert_eq!(q_rint(just_under), 0);
    assert_eq!(q_rint(-just_under), 0);
    assert_eq!(super::q_rint_f64(0.5), 1);
}

// spec CLIENT-9.2: svc_version changes the protocol and keeps the flags
#[test]
fn a_version_keeps_the_flags() {
    let changed = Protocol::RMQ.with_version(ProtocolVersion::FitzQuake);
    assert_eq!(changed.version(), ProtocolVersion::FitzQuake);
    assert_eq!(changed.flags(), Protocol::RMQ.flags());
}
