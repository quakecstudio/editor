// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! Unit tests for the vanilla builtins and the table.
//!
//! The math and string vectors are the test tables of `docs/spec/builtins.md`; each test cites
//! the statement it pins. Floats are compared bit for bit, as the spec gives the exact f32.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed lookup is a test failure"
)]

use crate::math;
use crate::table::{self, BUILTIN_MAX_NUMBER, Class};

/// Compares two f32 by bits, so NaNs and signed zeros are distinguished (DIFF-9.4).
fn same_bits(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits()
}

fn vec_same(a: [f32; 3], b: [f32; 3]) -> bool {
    same_bits(a[0], b[0]) && same_bits(a[1], b[1]) && same_bits(a[2], b[2])
}

// spec BUILTIN-5.1: the table has builtins 1 to 78, with the PF_Fixme gaps absent.
#[test]
fn table_has_the_vanilla_numbers() {
    // The present numbers: 1 to 78 minus the gaps (BUILTIN-1.2).
    let gaps = [5u32, 33, 39, 42, 50, 60, 61, 62, 63, 64, 65, 66, 71];
    for number in 1..=BUILTIN_MAX_NUMBER {
        let present = table::lookup(number).is_some();
        if gaps.contains(&number) {
            assert!(!present, "number {number} should be a PF_Fixme gap");
            assert!(table::is_fixme(number), "{number} should be is_fixme");
        } else {
            assert!(present, "number {number} should be a builtin");
            assert!(!table::is_fixme(number), "{number} should not be is_fixme");
        }
    }
}

// spec BUILTIN-5.2: three numbers share a C function under two names.
#[test]
fn precache_variants_share_a_c_function() {
    assert_eq!(table::lookup(75).unwrap().c_function, "PF_precache_model");
    assert_eq!(table::lookup(76).unwrap().c_function, "PF_precache_sound");
    assert_eq!(table::lookup(77).unwrap().c_function, "PF_precache_file");
    assert_eq!(table::lookup(75).unwrap().name, "precache_model2");
}

// spec BUILTIN-4.4: the class world builtins are the ones a mock cannot reproduce.
#[test]
fn world_builtins_have_the_world_class() {
    for number in [16u32, 17, 32, 34, 40, 41, 44, 67] {
        assert_eq!(table::lookup(number).unwrap().class, Class::World);
    }
}

// spec BUILTIN-7.1: ftos of a whole value is "%d"; a fraction is "%5.1f".
#[test]
fn ftos_whole_and_fraction() {
    assert_eq!(math::ftos(0.0), "0");
    assert_eq!(math::ftos(-0.0), "0");
    assert_eq!(math::ftos(1.0), "1");
    assert_eq!(math::ftos(-5.0), "-5");
    assert_eq!(math::ftos(0.5), "  0.5");
    assert_eq!(math::ftos(1.25), "  1.2");
    assert_eq!(math::ftos(1.35), "  1.4");
    assert_eq!(math::ftos(0.05), "  0.1");
    assert_eq!(math::ftos(-0.04), " -0.0");
}

// spec BUILTIN-7.6: ftos of a NaN or infinity is fixed.
#[test]
fn ftos_nan_and_infinity() {
    assert_eq!(math::ftos(f32::NAN), "  nan");
    assert_eq!(math::ftos(f32::INFINITY), "  inf");
    assert_eq!(math::ftos(f32::NEG_INFINITY), " -inf");
}

// spec BUILTIN-7.2: vtos is '%5.1f %5.1f %5.1f' with the single quotes.
#[test]
fn vtos_formats_each_component() {
    assert_eq!(math::vtos([0.0, 0.0, 0.0]), "'  0.0   0.0   0.0'");
    assert_eq!(math::vtos([1.0, -2.0, 3.5]), "'  1.0  -2.0   3.5'");
}

// spec BUILTIN-7.7: vtos of NaN, infinity, minus infinity.
#[test]
fn vtos_nan_and_infinity() {
    assert_eq!(
        math::vtos([f32::NAN, f32::INFINITY, f32::NEG_INFINITY]),
        "'  nan   inf  -inf'"
    );
}

// spec BUILTIN-6.7: rint rounds halves away from zero; 0 and -0 give +0.
#[test]
fn rint_rounds_halves_away_from_zero() {
    assert!(same_bits(math::rint(0.0), 0.0));
    assert!(same_bits(math::rint(-0.0), 0.0));
    assert_eq!(math::rint(0.5), 1.0);
    assert_eq!(math::rint(-0.5), -1.0);
    assert_eq!(math::rint(1.5), 2.0);
    assert_eq!(math::rint(2.5), 3.0);
    assert_eq!(math::rint(-2.5), -3.0);
    assert_eq!(math::rint(0.499_999_97), 0.0);
    assert_eq!(math::rint(-1.7), -2.0);
    assert_eq!(math::rint(3.2), 3.0);
}

// spec BUILTIN-6.8: floor, ceil keep a zero's sign; fabs clears the sign bit.
#[test]
fn floor_ceil_fabs() {
    assert_eq!(math::floor(-0.5), -1.0);
    assert!(same_bits(math::ceil(-0.5), -0.0));
    assert_eq!(math::ceil(1.2), 2.0);
    assert_eq!(math::floor(1.8), 1.0);
    assert!(same_bits(math::fabs(-0.0), 0.0));
}

// spec BUILTIN-6.2: vlen is (f32)sqrt(x*x + y*y + z*z).
#[test]
fn vlen_values() {
    assert_eq!(math::vlen([3.0, 4.0, 0.0]), 5.0);
    assert_eq!(math::vlen([1.0, 1.0, 1.0]), 1.732_050_8);
    assert_eq!(math::vlen([0.0, 0.0, 0.0]), 0.0);
}

// spec BUILTIN-6.3: normalize is '0 0 0' at length 0, else v * (f32)(1/len).
#[test]
fn normalize_values() {
    assert!(vec_same(math::normalize([3.0, 4.0, 0.0]), [0.6, 0.8, 0.0]));
    assert!(vec_same(
        math::normalize([0.0, 0.0, -2.0]),
        [0.0, 0.0, -1.0]
    ));
    assert!(vec_same(math::normalize([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]));
}

// spec BUILTIN-6.4: vectoyaw is a whole yaw from 0 to 359.
#[test]
fn vectoyaw_values() {
    assert_eq!(math::vectoyaw([1.0, 0.0, 0.0]), 0.0);
    assert_eq!(math::vectoyaw([0.0, 1.0, 0.0]), 90.0);
    assert_eq!(math::vectoyaw([-1.0, 0.0, 0.0]), 180.0);
    assert_eq!(math::vectoyaw([0.0, -1.0, 0.0]), 270.0);
    assert_eq!(math::vectoyaw([1.0, -1.0, 0.0]), 315.0);
    assert_eq!(math::vectoyaw([1.0, 0.01, 0.0]), 0.0);
    assert_eq!(math::vectoyaw([-1.0, -0.01, 0.0]), 181.0);
    assert_eq!(math::vectoyaw([0.0, 0.0, 5.0]), 0.0);
}

// spec BUILTIN-6.5: vectoangles is 'pitch yaw 0', with '0 0 0' giving '270 0 0'.
#[test]
fn vectoangles_values() {
    assert!(vec_same(
        math::vectoangles([0.0, 0.0, 1.0]),
        [90.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([0.0, 0.0, -1.0]),
        [270.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([0.0, 0.0, 0.0]),
        [270.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([1.0, 0.0, 1.0]),
        [45.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([1.0, 0.0, -1.0]),
        [315.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([-3.0, 4.0, 5.0]),
        [45.0, 126.0, 0.0]
    ));
}

// spec BUILTIN-6.1: makevectors of zero angles gives forward '1 0 -0'.
#[test]
fn makevectors_zero_angles() {
    let (forward, right, up) = math::makevectors([0.0, 0.0, 0.0]);
    assert!(vec_same(forward, [1.0, 0.0, -0.0]));
    assert!(vec_same(right, [0.0, -1.0, -0.0]));
    assert!(vec_same(up, [0.0, 0.0, 1.0]));
}

// spec BUILTIN-15.5: anglemod wraps to 0 up to 360 and truncates to a multiple of 360/65536.
#[test]
fn anglemod_values() {
    assert_eq!(math::anglemod(0.0), 0.0);
    assert_eq!(math::anglemod(20.0), 19.995_117);
    assert_eq!(math::anglemod(-90.0), 270.0);
    assert_eq!(math::anglemod(360.0), 0.0);
    assert_eq!(math::anglemod(45.5), 45.499_878);
    assert_eq!(math::anglemod(720.5), 0.499_877_93);
}

// spec BUILTIN-2.1: a float used as an integer truncates toward zero.
#[test]
fn float_to_int_truncates() {
    assert_eq!(math::float_to_int(1.9), 1);
    assert_eq!(math::float_to_int(-1.9), -1);
    assert_eq!(math::float_to_int(0.0), 0);
}

// spec BUILTIN-2.2: an out-of-range float or a NaN converts to 0x80000000.
#[test]
fn float_to_int_saturates() {
    assert_eq!(math::float_to_int(f32::NAN), i32::MIN);
    assert_eq!(math::float_to_int(1e30), i32::MIN);
    assert_eq!(math::float_to_int(-1e30), i32::MIN);
}

// spec BUILTIN-6.6: random of a 15-bit value is (r & 0x7fff) / (f32)0x7fff.
#[test]
fn random_bounds() {
    assert_eq!(math::random(32767), 1.0);
    assert_eq!(math::random(1), 3.051_851e-5);
    assert_eq!(math::random(0), 0.0);
}

// ===== the state, output and world builtin bodies (BUILTIN-8 to BUILTIN-16, DIFF-7) =====

mod bodies {
    //! Tests of the class state, output and world builtin bodies against the mock fixture.
    //!
    //! Each builds a progs whose entry function calls one builtin, runs it through a
    //! [`qc_vm::Machine`] with [`MockBuiltins`] over a [`MockEngine`], and checks the result,
    //! the edicts and the engine fixture. Each test cites the `builtins.md` or `differential.md`
    //! statement it pins.

    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::expect_used,
        clippy::arithmetic_side_effects,
        clippy::unwrap_in_result,
        reason = "test code: a failed unwrap, index, expect or overflow is a failed test"
    )]

    use crate::mock::{MessageDest, MockEngine};
    use crate::{BuiltinError, MockBuiltins};
    use qc_edict::system::{field_by_name, global_by_name};
    use qc_progs::opcode::Opcode;
    use qc_progs::{Function, Progs, ProgsFile, Statement, write};
    use qc_types::{PROGS_OFS_PARM0, PROGS_OFS_RETURN, PROGS_PARM_SLOTS, Slot};
    use qc_vm::{Machine, NoObserver, RunError, ServerState};

    /// A global slot holding the function value the entry's `CALL` calls.
    const CALL_TARGET_SLOT: i32 = 60;
    /// The entity field slots the progs has (EDICT-3.3 minimum); the mock adds none.
    const ENTITY_FIELDS: i32 = 105;

    /// A fixture that runs one builtin and gives access to the machine and engine after.
    struct Fixture {
        machine: Machine,
        builtins: MockBuiltins<MockEngine>,
        progs_bytes: Vec<u8>,
    }

    /// Builds a progs whose entry (function 1) calls builtin `number` with `argc` arguments,
    /// then is done. Function 2 is the builtin (`first_statement = -number`). `strings` is the
    /// string table bytes (a leading NUL for the empty string); `numglobals` is at least 92.
    fn build_progs(number: u32, argc: u16, strings: Vec<u8>) -> Vec<u8> {
        let mut globals = vec![Slot(0); 128];
        globals[usize::try_from(CALL_TARGET_SLOT).unwrap()] = Slot::from_i32(2);

        let call_number = 51_u32.saturating_add(u32::from(argc));
        let call_op = u32::from(Opcode::from_number(call_number).unwrap().number());
        let statements = vec![
            Statement {
                op: call_op,
                a: CALL_TARGET_SLOT,
                b: 0,
                c: 0,
            },
            Statement {
                op: u32::from(Opcode::Done.number()),
                a: i32::from(PROGS_OFS_RETURN),
                b: 0,
                c: 0,
            },
        ];
        let builtin_first = i32::try_from(number).unwrap().checked_neg().unwrap();
        let functions = vec![
            Function::default(),
            Function {
                first_statement: 0,
                ..Function::default()
            },
            Function {
                first_statement: builtin_first,
                ..Function::default()
            },
        ];
        let file = ProgsFile {
            entityfields: ENTITY_FIELDS,
            statements,
            functions,
            globals,
            strings,
            ..ProgsFile::default()
        };
        write(&file).unwrap()
    }

    /// A string table from a list of strings; returns the bytes and the offset of each.
    fn string_table(items: &[&str]) -> (Vec<u8>, Vec<i32>) {
        let mut bytes = vec![0_u8];
        let mut offsets = Vec::new();
        for item in items {
            offsets.push(i32::try_from(bytes.len()).unwrap());
            bytes.extend_from_slice(item.as_bytes());
            bytes.push(0);
        }
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        (bytes, offsets)
    }

    impl Fixture {
        /// A fixture for builtin `number`, `argc` args, a string table, and `maxclients`.
        fn new(number: u32, argc: u16, strings: Vec<u8>, maxclients: usize) -> Self {
            let progs_bytes = build_progs(number, argc, strings);
            let progs = Progs::read(&progs_bytes).unwrap();
            let machine = Machine::new(&progs, maxclients).unwrap();
            let mut engine = MockEngine::new("test", 0, 1);
            engine.clients_active = vec![true; maxclients];
            Self {
                machine,
                builtins: MockBuiltins::with_engine(engine),
                progs_bytes,
            }
        }

        /// Sets the `n`th parameter slot (`OFS_PARM0 + 3n`) to `value`.
        fn set_parm(&mut self, n: u16, value: Slot) {
            let slot = i32::from(PROGS_OFS_PARM0) + i32::from(PROGS_PARM_SLOTS) * i32::from(n);
            self.machine.set_global(slot, value).unwrap();
        }

        /// Sets the `n`th parameter to a float.
        fn set_parm_f(&mut self, n: u16, value: f32) {
            self.set_parm(n, Slot::from_f32(value));
        }

        /// Sets the `n`th parameter to a vector (three slots).
        fn set_parm_v(&mut self, n: u16, value: [f32; 3]) {
            let base = i32::from(PROGS_OFS_PARM0) + i32::from(PROGS_PARM_SLOTS) * i32::from(n);
            for (i, c) in value.into_iter().enumerate() {
                self.machine
                    .set_global(base + i32::try_from(i).unwrap(), Slot::from_f32(c))
                    .unwrap();
            }
        }

        /// Sets the `n`th parameter to a raw i32 (a string, entity or field value).
        fn set_parm_i(&mut self, n: u16, value: i32) {
            self.set_parm(n, Slot::from_i32(value));
        }

        /// Sets an entity-value system global (`self`, `msg_entity`) to edict `number`.
        fn set_global_entity(&mut self, name: &str, number: i32) {
            let value = self.machine.edicts().edict_num(number).unwrap();
            let slot = i32::try_from(global_by_name(name).unwrap().slot).unwrap();
            self.machine
                .set_global(slot, Slot::from_i32(value.0))
                .unwrap();
        }

        /// Sets a scalar field of edict `number` to a float.
        fn set_field_f(&mut self, number: usize, name: &str, value: f32) {
            let slot = field_by_name(name).unwrap().slot;
            self.machine
                .edicts_mut()
                .set_field(number, slot, Slot::from_f32(value));
        }

        /// Sets a vector field of edict `number`.
        fn set_field_v(&mut self, number: usize, name: &str, value: [f32; 3]) {
            let slot = field_by_name(name).unwrap().slot;
            for (i, c) in value.into_iter().enumerate() {
                self.machine
                    .edicts_mut()
                    .set_field(number, slot + i, Slot::from_f32(c));
            }
        }

        /// Reads a scalar field of edict `number` as a float.
        fn field_f(&self, number: usize, name: &str) -> f32 {
            let slot = field_by_name(name).unwrap().slot;
            self.machine.edicts().field(number, slot).unwrap().as_f32()
        }

        /// Reads a scalar field of edict `number` as an i32.
        fn field_i(&self, number: usize, name: &str) -> i32 {
            let slot = field_by_name(name).unwrap().slot;
            self.machine.edicts().field(number, slot).unwrap().as_i32()
        }

        /// Reads a vector field of edict `number`.
        fn field_v(&self, number: usize, name: &str) -> [f32; 3] {
            let slot = field_by_name(name).unwrap().slot;
            [
                self.machine.edicts().field(number, slot).unwrap().as_f32(),
                self.machine
                    .edicts()
                    .field(number, slot + 1)
                    .unwrap()
                    .as_f32(),
                self.machine
                    .edicts()
                    .field(number, slot + 2)
                    .unwrap()
                    .as_f32(),
            ]
        }

        /// The return slot as a float.
        fn return_f(&self) -> f32 {
            self.machine
                .global(i32::from(PROGS_OFS_RETURN))
                .unwrap()
                .as_f32()
        }

        /// The return slot as an i32.
        fn return_i(&self) -> i32 {
            self.machine
                .global(i32::from(PROGS_OFS_RETURN))
                .unwrap()
                .as_i32()
        }

        /// Runs the builtin; returns `Ok` or the run error.
        fn run(&mut self, server: ServerState) -> Result<(), RunError> {
            let progs = Progs::read(&self.progs_bytes).unwrap();
            self.machine
                .execute_program(&progs, server, &mut self.builtins, &mut NoObserver, 1)
        }

        /// Runs the builtin, expecting success.
        fn run_ok(&mut self, server: ServerState) {
            self.run(server).expect("builtin succeeded");
        }

        /// Runs the builtin, expecting a failure with this message.
        fn run_err(&mut self, server: ServerState, message: &str) {
            let error = self.run(server).expect_err("builtin failed");
            assert_eq!(error.message(), message);
        }

        /// The mock engine after the run.
        fn engine(&self) -> &MockEngine {
            self.builtins.engine()
        }
    }

    /// Spawns edict `number` (clears its free flag) so field access and links work.
    fn in_use(fixture: &mut Fixture, number: usize) {
        fixture.machine.edicts_mut().clear_edict(number);
    }

    // ----- BUILTIN-8: entities -----

    // spec BUILTIN-8.1: spawn allocates an edict and returns it.
    #[test]
    fn spawn_allocates_an_edict() {
        let mut f = Fixture::new(14, 0, vec![0], 0);
        let before = f.machine.edicts().num_edicts();
        f.run_ok(ServerState::Active);
        assert_eq!(f.machine.edicts().num_edicts(), before + 1);
        // The returned value names the new edict (number `before`).
        let value = f
            .machine
            .edicts()
            .edict_num(i32::try_from(before).unwrap())
            .unwrap();
        assert_eq!(f.return_i(), value.0);
        assert_eq!(f.machine.edicts().is_free(before), Some(false));
    }

    // spec BUILTIN-8.2: remove frees the edict, refusing nothing.
    #[test]
    fn remove_frees_the_edict() {
        let mut f = Fixture::new(15, 1, vec![0], 0);
        // num_edicts starts at 1 (world); grow it so edict 1 is in use.
        f.machine.edicts_mut().alloc(1.0).unwrap();
        in_use(&mut f, 1);
        let value = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, value.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.machine.edicts().is_free(1), Some(true));
    }

    // spec BUILTIN-8.4: nextent returns the first non-free edict after e, else the world.
    #[test]
    fn nextent_skips_free_edicts() {
        let mut f = Fixture::new(47, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1, in use
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2, in use
        f.machine.edicts_mut().free(1, 1.0); // free edict 1
        // nextent(world) starts at edict 1, which is free, so it reaches edict 2.
        f.set_parm_i(0, 0);
        f.run_ok(ServerState::Active);
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        assert_eq!(f.return_i(), e2.0);
    }

    // spec BUILTIN-8.3: find returns the first edict whose string field equals match.
    #[test]
    fn find_matches_a_string_field() {
        let (strings, offsets) = string_table(&["ab"]);
        let mut f = Fixture::new(18, 3, strings, 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2
        // classname (field slot 28) of edict 2 is "ab".
        let classname = field_by_name("classname").unwrap().slot;
        f.machine
            .edicts_mut()
            .set_field(2, classname, Slot::from_i32(offsets[0]));
        let world = f.machine.edicts().edict_num(0).unwrap();
        f.set_parm_i(0, world.0); // start
        f.set_parm_i(1, i32::try_from(classname).unwrap()); // field
        f.set_parm_i(2, offsets[0]); // match "ab"
        f.run_ok(ServerState::Active);
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        assert_eq!(f.return_i(), e2.0);
    }

    // spec BUILTIN-8.6: findradius chains the included edicts, newest first.
    #[test]
    fn findradius_chains_included_edicts() {
        let mut f = Fixture::new(22, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2
        for n in [1_usize, 2] {
            f.set_field_f(n, "solid", 1.0);
            f.set_field_v(n, "origin", [0.0, 0.0, 0.0]);
        }
        f.set_parm_v(0, [0.0, 0.0, 0.0]);
        f.set_parm_f(1, 100.0);
        f.run_ok(ServerState::Active);
        // The head is edict 2 (last included); its chain is edict 1; edict 1's chain is world.
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        assert_eq!(f.return_i(), e2.0);
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        let chain = field_by_name("chain").unwrap().slot;
        assert_eq!(f.machine.edicts().field(2, chain).unwrap().as_i32(), e1.0);
        assert_eq!(f.machine.edicts().field(1, chain).unwrap().as_i32(), 0);
    }

    // spec BUILTIN-8.7: setspawnparms of an edict outside 1..maxclients is a runtime error.
    #[test]
    fn setspawnparms_non_client_errors() {
        let mut f = Fixture::new(78, 1, vec![0], 1);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2, not a client slot
        in_use(&mut f, 2);
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        f.set_parm_i(0, e2.0);
        f.run_err(ServerState::Active, "Entity is not a client");
    }

    // ----- BUILTIN-9: placing -----

    // spec BUILTIN-9.1: setorigin sets origin and links (absmin = origin + mins - 1).
    #[test]
    fn setorigin_sets_and_links() {
        let mut f = Fixture::new(2, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.0);
        f.set_parm_v(1, [1.0, 2.0, 3.0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_v(1, "origin"), [1.0, 2.0, 3.0]);
        // mins are 0, widen 1: absmin = origin - 1.
        assert_eq!(f.field_v(1, "absmin"), [0.0, 1.0, 2.0]);
    }

    // spec BUILTIN-9.2: setsize of backwards mins/maxs is a runtime error before writing.
    #[test]
    fn setsize_backwards_errors() {
        let mut f = Fixture::new(4, 3, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.0);
        f.set_parm_v(1, [1.0, 0.0, 0.0]); // min
        f.set_parm_v(2, [0.0, 0.0, 0.0]); // max, backwards on x
        f.run_err(ServerState::Active, "backwards mins/maxs");
        assert_eq!(f.field_v(1, "mins"), [0.0, 0.0, 0.0]);
    }

    // spec BUILTIN-9.2: setsize sets mins, maxs, size and links.
    #[test]
    fn setsize_sets_fields() {
        let mut f = Fixture::new(4, 3, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.0);
        f.set_parm_v(1, [-16.0, -16.0, -24.0]);
        f.set_parm_v(2, [16.0, 16.0, 32.0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_v(1, "mins"), [-16.0, -16.0, -24.0]);
        assert_eq!(f.field_v(1, "size"), [32.0, 32.0, 56.0]);
    }

    // spec BUILTIN-9.3: setmodel of a name not precached is a runtime error.
    #[test]
    fn setmodel_not_precached_errors() {
        let (strings, offsets) = string_table(&["progs/x.mdl"]);
        let mut f = Fixture::new(3, 2, strings, 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.0);
        f.set_parm_i(1, offsets[0]);
        f.run_err(ServerState::Active, "no precache: progs/x.mdl\n");
    }

    // spec BUILTIN-9.4: setmodel sets model, modelindex and the .mdl bounds.
    #[test]
    fn setmodel_sets_bounds() {
        let (strings, offsets) = string_table(&["progs/x.mdl"]);
        let mut f = Fixture::new(3, 2, strings, 0);
        f.builtins.engine_mut().precache_model_name("progs/x.mdl");
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.0);
        f.set_parm_i(1, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_i(1, "model"), offsets[0]);
        assert_eq!(f.field_v(1, "mins"), [-16.0, -16.0, -16.0]);
        assert_eq!(f.field_v(1, "maxs"), [16.0, 16.0, 16.0]);
    }

    // ----- BUILTIN-10: precaching -----

    // spec BUILTIN-10.1: precache outside a spawn function (server active) is a runtime error.
    #[test]
    fn precache_sound_not_loading_errors() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut f = Fixture::new(19, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_err(
            ServerState::Active,
            "PF_Precache_*: Precache can only be done in spawn functions",
        );
    }

    // spec BUILTIN-10.2, BUILTIN-10.3: precache_sound returns s and adds it to the list.
    #[test]
    fn precache_sound_adds_and_returns() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut f = Fixture::new(19, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Loading);
        assert_eq!(f.return_i(), offsets[0]);
        assert_eq!(f.engine().sound_index("misc/x.wav"), Some(1));
    }

    // spec BUILTIN-10.2: an empty string precache is the runtime error Bad string.
    #[test]
    fn precache_sound_bad_string() {
        let mut f = Fixture::new(19, 1, vec![0], 0);
        f.set_parm_i(0, 0); // the empty string at offset 0
        f.run_err(ServerState::Loading, "Bad string");
    }

    // spec BUILTIN-10.5: precache_file returns s and does nothing, at any time.
    #[test]
    fn precache_file_returns_s() {
        let (strings, offsets) = string_table(&["maps/x.bsp"]);
        let mut f = Fixture::new(68, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_i(), offsets[0]);
    }

    // spec BUILTIN-10.6: precache_model2 behaves exactly as precache_model.
    #[test]
    fn precache_model2_adds() {
        let (strings, offsets) = string_table(&["progs/x.mdl"]);
        let mut f = Fixture::new(75, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Loading);
        assert_eq!(f.engine().model_index("progs/x.mdl"), Some(2));
    }

    // ----- BUILTIN-11: printing, commands, settings -----

    // spec BUILTIN-11.4: stuffcmd to a non-client is the runtime error Parm 0 not a client.
    #[test]
    fn stuffcmd_non_client_errors() {
        let (strings, offsets) = string_table(&["cmd\n"]);
        let mut f = Fixture::new(21, 2, strings, 1);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2, not a client
        in_use(&mut f, 2);
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        f.set_parm_i(0, e2.0);
        f.set_parm_i(1, offsets[0]);
        f.run_err(ServerState::Active, "Parm 0 not a client");
    }

    // spec BUILTIN-11.5: localcmd appends s to the command buffer.
    #[test]
    fn localcmd_appends() {
        let (strings, offsets) = string_table(&["quit\n"]);
        let mut f = Fixture::new(46, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().command_buffer, b"quit\n");
    }

    // spec BUILTIN-11.6: changelevel records it and appends; a second one does nothing.
    #[test]
    fn changelevel_once() {
        let (strings, offsets) = string_table(&["e1m2"]);
        let mut f = Fixture::new(70, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Active);
        assert!(f.engine().changelevel_issued);
        assert_eq!(f.engine().command_buffer, b"changelevel e1m2\n");
    }

    // spec BUILTIN-11.7: cvar returns the setting's value, or 0 for an unknown name.
    #[test]
    fn cvar_reads_a_setting() {
        let (strings, offsets) = string_table(&["skill", "nope"]);
        let mut f = Fixture::new(45, 1, strings, 0);
        f.builtins.engine_mut().cvars.set("skill", "2.5");
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 2.5);
        // An unknown name reads 0.
        let mut f2 = Fixture::new(45, 1, string_table(&["nope"]).0, 0);
        f2.set_parm_i(0, string_table(&["nope"]).1[0]);
        f2.run_ok(ServerState::Active);
        assert_eq!(f2.return_f(), 0.0);
    }

    // spec BUILTIN-11.8: cvar_set sets a known setting and leaves an unknown one unchanged.
    #[test]
    fn cvar_set_sets_a_setting() {
        let (strings, offsets) = string_table(&["skill", "3"]);
        let mut f = Fixture::new(72, 2, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.set_parm_i(1, offsets[1]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().cvars.text("skill"), Some("3"));
    }

    // ----- BUILTIN-12: sounds, particles, lightstyles, statics -----

    // spec BUILTIN-12.1: a volume outside 0..255 is a fatal error.
    #[test]
    fn sound_bad_volume() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut f = Fixture::new(8, 5, strings, 0);
        f.builtins.engine_mut().precache_sound_name("misc/x.wav");
        f.set_parm_f(1, 0.0); // channel
        f.set_parm_i(2, offsets[0]); // sample
        f.set_parm_f(3, 2.0); // volume * 255 = 510, out of range
        f.set_parm_f(4, 1.0); // attenuation
        f.run_err(ServerState::Active, "SV_StartSound: volume = 510");
    }

    // spec BUILTIN-12.3: a valid sound appends bytes to the unreliable datagram.
    #[test]
    fn sound_appends_to_datagram() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut f = Fixture::new(8, 5, strings, 0);
        f.builtins.engine_mut().precache_sound_name("misc/x.wav");
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.0);
        f.set_parm_f(1, 0.0);
        f.set_parm_i(2, offsets[0]);
        f.set_parm_f(3, 1.0);
        f.set_parm_f(4, 1.0);
        f.run_ok(ServerState::Active);
        assert!(f.engine().buffers.unreliable > 0);
    }

    // spec BUILTIN-12.6: lightstyle of a style outside 0..63 is invalid lightstyle.
    #[test]
    fn lightstyle_out_of_range() {
        let (strings, offsets) = string_table(&["a"]);
        let mut f = Fixture::new(35, 2, strings, 0);
        f.set_parm_f(0, 64.0);
        f.set_parm_i(1, offsets[0]);
        f.run_err(ServerState::Active, "invalid lightstyle");
    }

    // spec BUILTIN-12.6: lightstyle stores the value.
    #[test]
    fn lightstyle_stores() {
        let (strings, offsets) = string_table(&["mmnn"]);
        let mut f = Fixture::new(35, 2, strings, 0);
        f.set_parm_f(0, 3.0);
        f.set_parm_i(1, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().lightstyles[3].as_deref(), Some("mmnn"));
    }

    // spec BUILTIN-12.8: makestatic appends to the signon buffer and frees e.
    #[test]
    fn makestatic_frees_e() {
        let mut f = Fixture::new(69, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        // model field 0 means the empty model (index 0), which is allowed.
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.0);
        f.run_ok(ServerState::Active);
        assert!(f.engine().buffers.signon > 0);
        assert_eq!(f.machine.edicts().is_free(1), Some(true));
    }

    // ----- BUILTIN-13: network messages -----

    // spec BUILTIN-13.1: a bad destination is WriteDest: bad destination.
    #[test]
    fn write_bad_destination() {
        let mut f = Fixture::new(52, 2, vec![0], 0);
        f.set_parm_f(0, 5.0); // no such destination
        f.set_parm_f(1, 1.0);
        f.run_err(ServerState::Active, "WriteDest: bad destination");
    }

    // spec BUILTIN-13.2: WriteLong appends four bytes to the chosen buffer.
    #[test]
    fn write_long_four_bytes() {
        let mut f = Fixture::new(55, 2, vec![0], 0);
        f.set_parm_f(0, 0.0); // MSG_BROADCAST
        f.set_parm_f(1, 1.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.unreliable, 4);
    }

    // spec BUILTIN-13.1: MSG_ONE to a non-client is WriteDest: not a client.
    #[test]
    fn write_one_non_client() {
        let mut f = Fixture::new(52, 2, vec![0], 1);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2
        in_use(&mut f, 2);
        f.set_global_entity("msg_entity", 2); // not a client slot
        f.set_parm_f(0, 1.0); // MSG_ONE
        f.set_parm_f(1, 1.0);
        f.run_err(ServerState::Active, "WriteDest: not a client");
    }

    // spec BUILTIN-2.10: WriteString reads the destination before the string.
    #[test]
    fn write_string_bad_destination_first() {
        let mut f = Fixture::new(58, 2, vec![0], 0);
        f.set_parm_f(0, 5.0); // bad destination
        f.set_parm_i(1, 0x0100_0000); // also a bad string
        f.run_err(ServerState::Active, "WriteDest: bad destination");
    }

    // ----- BUILTIN-16: errors -----

    // spec BUILTIN-16.1: error raises a failure carrying the joined text.
    #[test]
    fn error_carries_text() {
        let (strings, offsets) = string_table(&["boom"]);
        let mut f = Fixture::new(10, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_err(ServerState::Active, "boom");
    }

    // spec BUILTIN-16.2: objerror frees self and carries the joined text.
    #[test]
    fn objerror_frees_self() {
        let (strings, offsets) = string_table(&["oops"]);
        let mut f = Fixture::new(11, 1, strings, 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        f.set_global_entity("self", 1);
        f.set_parm_i(0, offsets[0]);
        f.run_err(ServerState::Active, "oops");
        assert_eq!(f.machine.edicts().is_free(1), Some(true));
    }

    // ----- BUILTIN-2: argument checks (the DIFF-6.6 messages) -----

    // spec BUILTIN-2.7: an entity value that names no edict is invalid entity.
    #[test]
    fn invalid_entity_message() {
        let mut f = Fixture::new(47, 1, vec![0], 0); // nextent
        f.set_parm_i(0, 3); // 3 is not a whole edict (edict_size is 516), names no edict
        f.run_err(ServerState::Active, "invalid entity");
    }

    // spec BUILTIN-2.5: a string value neither in the table nor an engine string is invalid
    // string.
    #[test]
    fn invalid_string_message() {
        let mut f = Fixture::new(23, 1, vec![0], 0); // bprint
        f.set_parm_i(0, 0x0100_0000); // far outside the one-byte string table
        f.run_err(ServerState::Active, "invalid string");
    }

    // spec BUILTIN-8.3: a field index outside the progs' fields is invalid field.
    #[test]
    fn invalid_field_message() {
        let (strings, offsets) = string_table(&["x"]);
        let mut f = Fixture::new(18, 3, strings, 0); // find
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1, so find examines it
        let world = f.machine.edicts().edict_num(0).unwrap();
        f.set_parm_i(0, world.0);
        f.set_parm_i(1, 500); // 500 >= entityfields (105): invalid field
        f.set_parm_i(2, offsets[0]);
        f.run_err(ServerState::Active, "invalid field");
    }

    // spec BUILTIN-2.4: an entity at or above num_edicts is NUM_FOR_EDICT: bad pointer.
    #[test]
    fn num_for_edict_bad_pointer() {
        let mut f = Fixture::new(47, 1, vec![0], 0); // nextent reads with G_EDICTNUM
        // edict 10 is a whole edict but at/above num_edicts (1): the fatal error.
        let value = 10 * f.machine.edicts().layout().edict_size();
        f.set_parm_i(0, value);
        f.run_err(ServerState::Active, "NUM_FOR_EDICT: bad pointer");
    }

    // ----- DIFF-7: world builtins -----

    // spec DIFF-7.1: traceline hits nothing; trace_endpos is a copy of PARM1.
    #[test]
    fn traceline_writes_trace_globals() {
        let mut f = Fixture::new(16, 4, vec![0], 0);
        f.set_parm_v(0, [1.0, 2.0, 3.0]);
        f.set_parm_v(1, [4.0, 5.0, 6.0]);
        f.set_parm_f(2, 0.0);
        f.set_parm_i(3, 0); // forent: the world
        f.run_ok(ServerState::Active);
        let endpos = global_by_name("trace_endpos").unwrap().slot;
        let slot = i32::try_from(endpos).unwrap();
        assert_eq!(f.machine.global(slot).unwrap().as_f32(), 4.0);
        assert_eq!(f.machine.global(slot + 1).unwrap().as_f32(), 5.0);
        let frac = global_by_name("trace_fraction").unwrap().slot;
        assert_eq!(
            f.machine
                .global(i32::try_from(frac).unwrap())
                .unwrap()
                .as_f32(),
            1.0
        );
    }

    // spec DIFF-7.2: pointcontents returns -1.
    #[test]
    fn pointcontents_returns_empty() {
        let mut f = Fixture::new(41, 1, vec![0], 0);
        f.set_parm_v(0, [0.0, 0.0, -1.0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), -1.0);
    }

    // spec DIFF-7.4: checkbottom returns 1; aim returns a copy of v_forward.
    #[test]
    fn checkbottom_returns_one() {
        let mut f = Fixture::new(40, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 1.0);
    }

    // spec DIFF-7.5: droptofloor links self, sets FL_ONGROUND and groundentity, returns 1.
    #[test]
    fn droptofloor_sets_onground() {
        let mut f = Fixture::new(34, 0, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        f.set_field_f(1, "flags", 1.0);
        f.set_field_v(1, "origin", [0.0, 0.0, 100.0]);
        f.set_global_entity("self", 1);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 1.0);
        // flags 1 | FL_ONGROUND (512) = 513.
        assert_eq!(f.field_f(1, "flags"), 513.0);
        assert_eq!(f.field_i(1, "groundentity"), 0);
        // origin unchanged; absmin z = 100 + 0 - 1 = 99.
        assert_eq!(f.field_v(1, "origin")[2], 100.0);
        assert_eq!(f.field_v(1, "absmin")[2], 99.0);
    }

    // spec DIFF-7.7: walkmove without a movement flag returns 0 and moves nothing.
    #[test]
    fn walkmove_without_flags() {
        let mut f = Fixture::new(32, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 4096.0); // no FL_ONGROUND/FL_FLY/FL_SWIM
        f.set_field_v(1, "origin", [10.0, 20.0, 30.0]);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 0.0);
        f.set_parm_f(1, 10.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 0.0);
        assert_eq!(f.field_v(1, "origin"), [10.0, 20.0, 30.0]);
    }

    // spec DIFF-7.6, DIFF-7.7: walkmove on the ground steps in x and y and returns 1.
    #[test]
    fn walkmove_steps() {
        let mut f = Fixture::new(32, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 512.0); // FL_ONGROUND
        f.set_field_v(1, "origin", [10.0, 20.0, 30.0]);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 180.0); // yaw
        f.set_parm_f(1, 10.0); // dist
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 1.0);
        // yaw 180 by 10 moves x by -10: from 10 to 0 (the sine's residue is below a float
        // step at y=20).
        assert_eq!(f.field_v(1, "origin")[0], 0.0);
        assert_eq!(f.field_v(1, "origin")[1], 20.0);
    }

    // spec DIFF-7.8: movetogoal without a movement flag writes 0 to RETURN.
    #[test]
    fn movetogoal_without_flags() {
        let mut f = Fixture::new(67, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 0.0);
        f.set_field_v(1, "origin", [10.0, 20.0, 30.0]);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 8.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 0.0);
        assert_eq!(f.field_v(1, "origin"), [10.0, 20.0, 30.0]);
    }

    // spec DIFF-6.6: the four argument-check messages are the mock's runtime errors; a message
    // destination enum round-trips its variants (used by the Write builtins).
    #[test]
    fn message_dest_variants() {
        for dest in [
            MessageDest::Broadcast,
            MessageDest::One,
            MessageDest::All,
            MessageDest::Init,
        ] {
            assert_eq!(dest, dest);
        }
        // A typed engine-only error still reports its message (BUILTIN-4.4).
        let error = BuiltinError::EngineOnly {
            number: 16,
            name: "traceline",
        };
        assert_eq!(error.message(), "builtin 16 (traceline) needs an engine");
    }
}
