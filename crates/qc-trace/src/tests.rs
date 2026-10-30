// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use proptest::prelude::*;

use super::{
    Changes, FieldChange, GlobalChange, Header, JsonFault, Record, Step, StringValue,
    TRACE_HEADER_SIZE, TRACE_MAGIC, TRACE_TAG_ERROR, TRACE_TAG_EXIT, TRACE_TAG_RUN, TRACE_TAG_STEP,
    TRACE_TAG_STRING, TRACE_UVAR_MAX_BYTES, TRACE_VERSION, Text, Trace, TraceError, decode, encode,
    encode_header, encode_record, from_json, header_to_json, record_to_json, to_json,
};

const SAMPLE_JSONL: &str = include_str!("../../../docs/spec/vectors/trace/sample.jsonl");
const SAMPLE_HEX: &str = include_str!("../../../docs/spec/vectors/trace/sample.hex");

/// Parses the vectors' hex format: hex byte pairs, `#` comments to the end of the line.
fn parse_hex(text: &str) -> Vec<u8> {
    text.lines()
        .flat_map(|line| line.split('#').next().unwrap().split_whitespace())
        .map(|pair| u8::from_str_radix(pair, 16).unwrap())
        .collect()
}

/// Fields of a test header; `Default` is the ordinary case.
struct HeaderSpec {
    progs_crc: u16,
    edict_size: u32,
    field_offset: u32,
    entityfields: u32,
    numstrings: u32,
}

impl Default for HeaderSpec {
    fn default() -> Self {
        Self {
            progs_crc: 12345,
            edict_size: 516,
            field_offset: 96,
            entityfields: 105,
            numstrings: 16,
        }
    }
}

fn header(spec: HeaderSpec) -> Header {
    Header::new(
        spec.progs_crc,
        spec.edict_size,
        spec.field_offset,
        spec.entityfields,
        spec.numstrings,
    )
    .unwrap()
}

fn globals(pairs: &[(u32, u32)]) -> Changes {
    Changes::new(
        pairs
            .iter()
            .map(|&(slot, bits)| GlobalChange { slot, bits })
            .collect(),
        Vec::new(),
    )
    .unwrap()
}

fn fields(triples: &[(u32, u32, u32)]) -> Changes {
    let list = triples
        .iter()
        .map(|&(edict, field, bits)| FieldChange { edict, field, bits });
    Changes::new(Vec::new(), list.collect()).unwrap()
}

fn text(bytes: &[u8]) -> Text {
    Text::new(bytes).unwrap()
}

fn step(
    function: i32,
    statement: i32,
    opcode: u16,
    builtin: Option<u32>,
    changes: Changes,
) -> Record {
    Record::Step(Step {
        function,
        statement,
        opcode,
        builtin,
        changes,
    })
}

/// The trace in `docs/spec/vectors/trace/`, built from its values.
fn sample() -> Trace {
    Trace {
        header: header(HeaderSpec::default()),
        records: vec![
            Record::Run {
                function: 1,
                changes: globals(&[(31, 0x3f80_0000)]),
            },
            step(1, 1, 6, None, globals(&[(100, 0x4000_0000)])),
            Record::String {
                value: 0x100,
                text: text(b"a\\b\n"),
            },
            Record::Run {
                function: 2,
                changes: globals(&[(1, 0x100)]),
            },
            step(2, 5, 37, None, fields(&[(2, 44, 2), (1, 30, 0x3f80_0000)])),
            step(2, 6, 0, None, globals(&[(1, 0)])),
            Record::Exit {
                changes: Changes::default(),
            },
            step(1, 2, 52, Some(2), globals(&[(1, 0x204)])),
            Record::Error {
                function: 1,
                statement: 3,
                message: text(b"Bad opcode 70"),
                changes: Changes::default(),
            },
            Record::Run {
                function: -5,
                changes: Changes::default(),
            },
            Record::Error {
                function: -5,
                statement: -1,
                message: text(b"PR_ExecuteProgram: NULL function"),
                changes: Changes::default(),
            },
        ],
    }
}

fn header_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    encode_header(&header(HeaderSpec::default()), &mut out);
    out
}

/// A binary trace with the default header and the given record bytes after it.
fn with_records(records: &[u8]) -> Vec<u8> {
    let mut out = header_bytes();
    out.extend_from_slice(records);
    out
}

fn record_bytes(record: &Record) -> Vec<u8> {
    let mut out = Vec::new();
    encode_record(record, &mut out);
    out
}

/// The JSON form with the default header and the given record lines after it.
fn json_with(lines: &[&str]) -> String {
    let mut out = header_to_json(&header(HeaderSpec::default()));
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn json_line_error(lines: &[&str]) -> TraceError {
    from_json(&json_with(lines)).unwrap_err()
}

fn is_json_error(error: &TraceError, line: usize) -> bool {
    matches!(error, TraceError::Json { line: l, .. } if *l == line)
}

// ---------------------------------------------------------------------------------------
// Vectors

// spec TRACE-1.1, TRACE-5.4, TRACE-5.5, TRACE-5.6: the sample's bytes are the header then each record
#[test]
fn binary_vector_encodes_and_decodes() {
    let bytes = parse_hex(SAMPLE_HEX);
    assert_eq!(encode(&sample()), bytes);
    assert_eq!(decode(&bytes).unwrap(), sample());
}

// spec TRACE-6.1, TRACE-6.2, TRACE-6.3: the sample's JSON lines are the header then each record
#[test]
fn json_vector_encodes_and_decodes() {
    assert_eq!(to_json(&sample()), SAMPLE_JSONL);
    assert_eq!(from_json(SAMPLE_JSONL).unwrap(), sample());
}

// spec TRACE-1.2: both vector files hold the same trace
#[test]
fn vector_forms_agree() {
    assert_eq!(
        decode(&parse_hex(SAMPLE_HEX)).unwrap(),
        from_json(SAMPLE_JSONL).unwrap()
    );
}

// spec TRACE-7.1, TRACE-7.3: the sample's CALL step returns edict 1 and its string record names 0x100
#[test]
fn vector_values_interpret() {
    let trace = sample();
    assert_eq!(trace.header.entity_number(0x204), Some(1));
    assert_eq!(
        trace.string_value(4, 0x100),
        StringValue::Engine(&text(b"a\\b\n"))
    );
    assert_eq!(trace.string_value(2, 0x100), StringValue::Unknown);
}

// ---------------------------------------------------------------------------------------
// TRACE-1 and TRACE-2

// spec TRACE-1.1: a trace with no records is the header alone, in both forms
#[test]
fn header_only_trace() {
    let trace = Trace {
        header: header(HeaderSpec::default()),
        records: Vec::new(),
    };
    let bytes = encode(&trace);
    assert_eq!(bytes.len(), TRACE_HEADER_SIZE);
    assert_eq!(decode(&bytes).unwrap(), trace);
    assert_eq!(from_json(&to_json(&trace)).unwrap(), trace);
}

// spec TRACE-1.1: there is no end marker, so a trace may stop after any complete record
#[test]
fn trace_ends_after_any_complete_record() {
    let full = encode(&sample());
    let mut end = TRACE_HEADER_SIZE;
    for (count, record) in sample().records.iter().enumerate() {
        end += record_bytes(record).len();
        let decoded = decode(&full[..end]).unwrap();
        assert_eq!(decoded.records.len(), count + 1);
    }
    assert_eq!(end, full.len());
}

// spec TRACE-1.4: values are kept bit for bit, NaN payloads and negative zero included
#[test]
fn values_keep_every_bit() {
    let patterns = [
        0x7fc0_0000,
        0xffc0_0001,
        0x7f80_0001,
        0x8000_0000,
        0xffff_ffff,
        0,
    ];
    let pairs: Vec<(u32, u32)> = patterns
        .iter()
        .zip(0..)
        .map(|(&bits, slot)| (slot, bits))
        .collect();
    let trace = Trace {
        header: header(HeaderSpec::default()),
        records: vec![Record::Exit {
            changes: globals(&pairs),
        }],
    };
    assert_eq!(decode(&encode(&trace)).unwrap(), trace);
    assert_eq!(from_json(&to_json(&trace)).unwrap(), trace);
}

// spec TRACE-1.4: edicts, fields and globals are named by number, not by entity value
#[test]
fn changes_name_slots_by_number() {
    let changes = fields(&[(599, 104, 1)]);
    let line = record_to_json(&Record::Exit { changes });
    assert_eq!(
        line,
        "{\"t\":\"exit\",\"globals\":[],\"fields\":[[599,104,\"00000001\"]]}\n"
    );
}

// spec TRACE-2.1: the header keeps each of its five numbers
#[test]
fn header_keeps_its_numbers() {
    let h = header(HeaderSpec {
        progs_crc: 0xffff,
        edict_size: 1000,
        field_offset: 4,
        entityfields: 7,
        numstrings: 9,
    });
    assert_eq!(
        (
            h.progs_crc(),
            h.edict_size(),
            h.field_offset(),
            h.entityfields(),
            h.numstrings()
        ),
        (0xffff, 1000, 4, 7, 9)
    );
    let trace = Trace {
        header: h,
        records: Vec::new(),
    };
    assert_eq!(decode(&encode(&trace)).unwrap().header, h);
    assert_eq!(from_json(&to_json(&trace)).unwrap().header, h);
}

// spec TRACE-2.2: a reader rejects any version other than TRACE_VERSION
#[test]
fn other_versions_are_rejected() {
    let mut bytes = header_bytes();
    bytes[4] = 2;
    assert_eq!(decode(&bytes), Err(TraceError::UnsupportedVersion(2)));
    bytes[4] = 0;
    assert_eq!(decode(&bytes), Err(TraceError::UnsupportedVersion(0)));
    let json =
        header_to_json(&header(HeaderSpec::default())).replace("\"version\":1", "\"version\":2");
    assert_eq!(from_json(&json), Err(TraceError::UnsupportedVersion(2)));
    assert_eq!(TRACE_VERSION, 1);
}

// spec TRACE-2.3: edict_size is at least 1 and the fields fit inside an edict
#[test]
fn header_layout_is_checked() {
    assert_eq!(Header::new(0, 0, 0, 0, 0), Err(TraceError::BadHeader));
    assert!(Header::new(0, 1, 0, 0, 0).is_ok());
    assert!(Header::new(0, 516, 96, 105, 0).is_ok());
    assert_eq!(Header::new(0, 515, 96, 105, 0), Err(TraceError::BadHeader));
    assert_eq!(Header::new(0, 516, 97, 105, 0), Err(TraceError::BadHeader));
    assert_eq!(
        Header::new(0, u32::MAX, 0, u32::MAX, 0),
        Err(TraceError::BadHeader)
    );
    assert_eq!(
        Header::new(0, u32::MAX, u32::MAX, 1, 0),
        Err(TraceError::BadHeader)
    );
    assert!(Header::new(0, u32::MAX, 3, (u32::MAX - 3) / 4, 0).is_ok());
}

// spec TRACE-2.3: a reader rejects a header that breaks the layout rules or a CRC above 16 bits
#[test]
fn readers_reject_bad_headers() {
    let mut bytes = header_bytes();
    bytes[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(decode(&bytes), Err(TraceError::BadHeader));
    let mut bytes = header_bytes();
    bytes[8..12].copy_from_slice(&0x1_0000u32.to_le_bytes());
    assert_eq!(decode(&bytes), Err(TraceError::BadHeader));
    let line = header_to_json(&header(HeaderSpec::default()));
    let json = line.replace("\"edict_size\":516", "\"edict_size\":515");
    assert_eq!(from_json(&json), Err(TraceError::BadHeader));
    let json = line.replace("\"progs_crc\":12345", "\"progs_crc\":65536");
    assert_eq!(from_json(&json), Err(TraceError::BadHeader));
}

// ---------------------------------------------------------------------------------------
// TRACE-3

// spec TRACE-3.2: a change set lists each slot at most once
#[test]
fn duplicate_changes_are_rejected() {
    let g = |slot, bits| GlobalChange { slot, bits };
    let f = |edict, field, bits| FieldChange { edict, field, bits };
    assert_eq!(
        Changes::new(vec![g(1, 0), g(1, 1)], Vec::new()),
        Err(TraceError::DuplicateChange)
    );
    assert_eq!(
        Changes::new(Vec::new(), vec![f(1, 2, 0), f(1, 2, 0)]),
        Err(TraceError::DuplicateChange)
    );
    assert!(
        Changes::new(
            vec![g(1, 0), g(2, 0)],
            vec![f(1, 2, 0), f(2, 1, 0), f(1, 3, 0)]
        )
        .is_ok()
    );
    let dup_binary = with_records(&[TRACE_TAG_EXIT, 2, 5, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0]);
    assert_eq!(decode(&dup_binary), Err(TraceError::ChangesOutOfOrder));
    let err = json_line_error(&[
        "{\"t\":\"exit\",\"globals\":[[5,\"00000000\"],[5,\"00000000\"]],\"fields\":[]}",
    ]);
    assert_eq!(err, TraceError::ChangesOutOfOrder);
}

// spec TRACE-3.3: globals ascend by slot, fields by edict then field slot
#[test]
fn changes_are_sorted() {
    let changes = Changes::new(
        vec![
            GlobalChange { slot: 9, bits: 0 },
            GlobalChange { slot: 3, bits: 1 },
        ],
        vec![
            FieldChange {
                edict: 2,
                field: 0,
                bits: 0,
            },
            FieldChange {
                edict: 1,
                field: 7,
                bits: 0,
            },
            FieldChange {
                edict: 1,
                field: 3,
                bits: 0,
            },
        ],
    )
    .unwrap();
    let slots: Vec<u32> = changes.globals().iter().map(|g| g.slot).collect();
    assert_eq!(slots, [3, 9]);
    let keys: Vec<(u32, u32)> = changes
        .fields()
        .iter()
        .map(|f| (f.edict, f.field))
        .collect();
    assert_eq!(keys, [(1, 3), (1, 7), (2, 0)]);
    assert!(!changes.is_empty());
    assert!(Changes::default().is_empty());
    assert!(!globals(&[(1, 0)]).is_empty());
    assert!(!fields(&[(1, 2, 0)]).is_empty());
}

// spec TRACE-3.3: a reader rejects changes out of order
#[test]
fn readers_reject_unsorted_changes() {
    let globals_desc = with_records(&[TRACE_TAG_EXIT, 2, 6, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0]);
    assert_eq!(decode(&globals_desc), Err(TraceError::ChangesOutOfOrder));
    let fields_desc = with_records(&[TRACE_TAG_EXIT, 0, 2, 2, 0, 0, 0, 0, 0, 1, 9, 0, 0, 0, 0]);
    assert_eq!(decode(&fields_desc), Err(TraceError::ChangesOutOfOrder));
    let err = json_line_error(&[
        "{\"t\":\"exit\",\"globals\":[],\"fields\":[[1,9,\"00000000\"],[1,8,\"00000000\"]]}",
    ]);
    assert_eq!(err, TraceError::ChangesOutOfOrder);
    let fields_dup = with_records(&[TRACE_TAG_EXIT, 0, 2, 1, 9, 0, 0, 0, 0, 1, 9, 0, 0, 0, 0]);
    assert_eq!(decode(&fields_dup), Err(TraceError::ChangesOutOfOrder));
}

// ---------------------------------------------------------------------------------------
// TRACE-4

// spec TRACE-4.10: a string record carries no change set
#[test]
fn string_records_carry_no_changes() {
    let string = Record::String {
        value: 7,
        text: text(b"x"),
    };
    assert_eq!(string.changes(), None);
    let exit = Record::Exit {
        changes: globals(&[(1, 1)]),
    };
    assert_eq!(exit.changes(), Some(&globals(&[(1, 1)])));
    let run = Record::Run {
        function: 1,
        changes: globals(&[(2, 2)]),
    };
    assert_eq!(run.changes(), Some(&globals(&[(2, 2)])));
    assert_eq!(
        step(1, 1, 0, None, globals(&[(3, 3)])).changes(),
        Some(&globals(&[(3, 3)]))
    );
    let error = Record::Error {
        function: 0,
        statement: 0,
        message: text(b""),
        changes: globals(&[(4, 4)]),
    };
    assert_eq!(error.changes(), Some(&globals(&[(4, 4)])));
}

// spec TRACE-4.8: a message, like any text, contains no NUL byte
#[test]
fn text_rejects_nul() {
    assert_eq!(Text::new(b"a\0b".as_slice()), Err(TraceError::NulInText));
    assert_eq!(text(&[0x01, 0x80, 0xff]).as_bytes(), [0x01, 0x80, 0xff]);
    let binary = with_records(&[TRACE_TAG_ERROR, 0, 0, 1, 0, 0, 0]);
    assert_eq!(decode(&binary), Err(TraceError::NulInText));
    let err = json_line_error(&[
        "{\"t\":\"error\",\"function\":0,\"statement\":0,\"message\":\"\\\\x00\",\"globals\":[],\"fields\":[]}",
    ]);
    assert_eq!(err, TraceError::NulInText);
}

// ---------------------------------------------------------------------------------------
// TRACE-5

/// The uvar encoding of `value`, taken from an exit record whose one global slot is `value`:
/// the tag, the count 1, the slot's uvar, 4 bytes of bits and the field count 0.
fn uvar_bytes(value: u32) -> Vec<u8> {
    let bytes = record_bytes(&Record::Exit {
        changes: globals(&[(value, 0)]),
    });
    bytes[2..bytes.len() - 5].to_vec()
}

// spec TRACE-5.1: uvars are LEB128, least significant group first
#[test]
fn uvar_encoding() {
    assert_eq!(uvar_bytes(0), [0x00]);
    assert_eq!(uvar_bytes(1), [0x01]);
    assert_eq!(uvar_bytes(127), [0x7f]);
    assert_eq!(uvar_bytes(128), [0x80, 0x01]);
    assert_eq!(uvar_bytes(300), [0xac, 0x02]);
    assert_eq!(uvar_bytes(16_384), [0x80, 0x80, 0x01]);
    assert_eq!(uvar_bytes(u32::MAX), [0xff, 0xff, 0xff, 0xff, 0x0f]);
    assert_eq!(TRACE_UVAR_MAX_BYTES, 5);
}

// spec TRACE-5.1: svars are zigzag, so 0, -1, 1, -2 become 0, 1, 2, 3
#[test]
fn svar_encoding() {
    let cases: [(i32, &[u8]); 7] = [
        (0, &[0x00]),
        (-1, &[0x01]),
        (1, &[0x02]),
        (-2, &[0x03]),
        (-64, &[0x7f]),
        (i32::MAX, &[0xfe, 0xff, 0xff, 0xff, 0x0f]),
        (i32::MIN, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
    ];
    for (n, expected) in cases {
        let bytes = record_bytes(&Record::Run {
            function: n,
            changes: Changes::default(),
        });
        assert_eq!(&bytes[1..bytes.len() - 2], expected, "svar {n}");
        assert_eq!(
            decode(&with_records(&bytes)).unwrap().records,
            [Record::Run {
                function: n,
                changes: Changes::default()
            }]
        );
    }
}

// spec TRACE-5.2: a uvar is at most 5 bytes, fits in 32 bits and is minimal
#[test]
fn readers_reject_bad_uvars() {
    let offset = TRACE_HEADER_SIZE + 1;
    let six = with_records(&[TRACE_TAG_RUN, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00, 0, 0]);
    assert_eq!(decode(&six), Err(TraceError::BadUvar { offset }));
    let too_big = with_records(&[TRACE_TAG_RUN, 0xff, 0xff, 0xff, 0xff, 0x10, 0, 0]);
    assert_eq!(decode(&too_big), Err(TraceError::BadUvar { offset }));
    let overlong = with_records(&[TRACE_TAG_RUN, 0x80, 0x00, 0, 0]);
    assert_eq!(decode(&overlong), Err(TraceError::BadUvar { offset }));
    let overlong_five = with_records(&[TRACE_TAG_RUN, 0x81, 0x80, 0x80, 0x80, 0x00, 0, 0]);
    assert_eq!(decode(&overlong_five), Err(TraceError::BadUvar { offset }));
    let max = with_records(&[TRACE_TAG_RUN, 0xff, 0xff, 0xff, 0xff, 0x0f, 0, 0]);
    assert!(decode(&max).is_ok());
    let unfinished = with_records(&[TRACE_TAG_RUN, 0x80]);
    assert_eq!(
        decode(&unfinished),
        Err(TraceError::Truncated {
            offset: TRACE_HEADER_SIZE
        })
    );
}

// spec TRACE-5.3: text is a uvar length and that many bytes, none of them NUL
#[test]
fn text_encoding() {
    let record = Record::String {
        value: 0x0102_0304,
        text: text(b"hi"),
    };
    assert_eq!(
        record_bytes(&record),
        [TRACE_TAG_STRING, 4, 3, 2, 1, 2, b'h', b'i']
    );
    let long = text(&[b'x'; 200]);
    let bytes = record_bytes(&Record::String {
        value: 0,
        text: long,
    });
    assert_eq!(&bytes[5..7], [0xc8, 0x01]);
    let nul = with_records(&[TRACE_TAG_STRING, 0, 0, 0, 0, 1, 0]);
    assert_eq!(decode(&nul), Err(TraceError::NulInText));
}

// spec TRACE-5.4: the header is the magic QCTR then six little-endian words
#[test]
fn header_encoding() {
    let bytes = header_bytes();
    assert_eq!(bytes.len(), TRACE_HEADER_SIZE);
    assert_eq!(&bytes[..4], TRACE_MAGIC);
    assert_eq!(TRACE_MAGIC, [0x51, 0x43, 0x54, 0x52]);
    let words: Vec<u32> = bytes[4..]
        .chunks(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    assert_eq!(words, [1, 12345, 516, 96, 105, 16]);
}

// spec TRACE-5.5: a change set is counted globals (slot, word) then counted fields (edict, field, word)
#[test]
fn change_set_encoding() {
    let changes = Changes::new(
        vec![GlobalChange {
            slot: 200,
            bits: 0xdead_beef,
        }],
        vec![FieldChange {
            edict: 3,
            field: 130,
            bits: 0x0102_0304,
        }],
    )
    .unwrap();
    assert_eq!(
        record_bytes(&Record::Exit { changes }),
        [
            TRACE_TAG_EXIT,
            1,
            0xc8,
            0x01,
            0xef,
            0xbe,
            0xad,
            0xde,
            1,
            3,
            0x82,
            0x01,
            4,
            3,
            2,
            1
        ]
    );
}

// spec TRACE-5.6: each record is its tag byte then its fields in the table's order
#[test]
fn record_encoding() {
    assert_eq!(
        record_bytes(&Record::Run {
            function: 3,
            changes: Changes::default()
        }),
        [TRACE_TAG_RUN, 6, 0, 0]
    );
    assert_eq!(
        record_bytes(&step(1, 2, 300, None, Changes::default())),
        [TRACE_TAG_STEP, 2, 4, 0xac, 0x02, 0, 0, 0]
    );
    assert_eq!(
        record_bytes(&step(1, 2, 51, Some(78), Changes::default())),
        [TRACE_TAG_STEP, 2, 4, 51, 1, 78, 0, 0]
    );
    assert_eq!(
        record_bytes(&Record::Exit {
            changes: Changes::default()
        }),
        [TRACE_TAG_EXIT, 0, 0]
    );
    let error = Record::Error {
        function: -1,
        statement: 0,
        message: text(b"x"),
        changes: Changes::default(),
    };
    assert_eq!(record_bytes(&error), [TRACE_TAG_ERROR, 1, 0, 1, b'x', 0, 0]);
    assert_eq!(
        record_bytes(&Record::String {
            value: 1,
            text: text(b"")
        }),
        [TRACE_TAG_STRING, 1, 0, 0, 0, 0]
    );
    assert_eq!(
        (
            TRACE_TAG_RUN,
            TRACE_TAG_STEP,
            TRACE_TAG_EXIT,
            TRACE_TAG_ERROR,
            TRACE_TAG_STRING
        ),
        (1, 2, 3, 4, 5)
    );
}

// spec TRACE-5.7: a reader rejects a short stream, a wrong magic, a cut record, a bad tag, flag or opcode
#[test]
fn readers_reject_malformed_streams() {
    assert_eq!(decode(&[]), Err(TraceError::Truncated { offset: 0 }));
    assert_eq!(decode(b"QCT"), Err(TraceError::Truncated { offset: 0 }));
    assert_eq!(
        decode(&header_bytes()[..27]),
        Err(TraceError::Truncated { offset: 0 })
    );
    assert_eq!(decode(b"QCTX"), Err(TraceError::BadMagic));
    let mut wrong = header_bytes();
    wrong[3] = b'S';
    assert_eq!(decode(&wrong), Err(TraceError::BadMagic));
    let full = record_bytes(&sample().records[4]);
    for cut in 1..full.len() {
        assert_eq!(
            decode(&with_records(&full[..cut])),
            Err(TraceError::Truncated {
                offset: TRACE_HEADER_SIZE
            }),
            "cut at {cut}"
        );
    }
    let at = TRACE_HEADER_SIZE;
    assert_eq!(
        decode(&with_records(&[0x00])),
        Err(TraceError::UnknownTag { offset: at, tag: 0 })
    );
    assert_eq!(
        decode(&with_records(&[0x06])),
        Err(TraceError::UnknownTag { offset: at, tag: 6 })
    );
    let flag = with_records(&[TRACE_TAG_STEP, 0, 0, 0, 2, 0, 0]);
    assert_eq!(
        decode(&flag),
        Err(TraceError::BadBuiltinFlag { offset: at + 4 })
    );
    let opcode = with_records(&[TRACE_TAG_STEP, 0, 0, 0x80, 0x80, 0x04, 0, 0, 0]);
    assert_eq!(decode(&opcode), Err(TraceError::OpcodeOutOfRange(65536)));
    let max_opcode = with_records(&[TRACE_TAG_STEP, 0, 0, 0xff, 0xff, 0x03, 0, 0, 0]);
    assert!(decode(&max_opcode).is_ok());
    let huge_count = with_records(&[TRACE_TAG_EXIT, 0xff, 0xff, 0xff, 0xff, 0x0f]);
    assert_eq!(
        decode(&huge_count),
        Err(TraceError::Truncated { offset: at })
    );
}

// ---------------------------------------------------------------------------------------
// TRACE-6

// spec TRACE-6.1: one object per line, each ended by a line feed, with no blank lines
#[test]
fn json_lines_framing() {
    let json = to_json(&sample());
    assert!(json.ends_with('\n'));
    assert_eq!(json.lines().count(), 12);
    let missing_feed = json.trim_end_matches('\n');
    assert!(is_json_error(&from_json(missing_feed).unwrap_err(), 12));
    let blank = json_with(&["", "{\"t\":\"exit\",\"globals\":[],\"fields\":[]}"]);
    assert!(is_json_error(&from_json(&blank).unwrap_err(), 2));
    assert!(is_json_error(&from_json("").unwrap_err(), 1));
    assert!(is_json_error(&from_json("\n").unwrap_err(), 1));
    assert!(is_json_error(&json_line_error(&["[1]"]), 2));
    assert!(is_json_error(&json_line_error(&["{"]), 2));
}

/// The fault of a JSON error at `line`, or `None` for any other error.
fn fault_at(error: TraceError, line: usize) -> Option<JsonFault> {
    match error {
        TraceError::Json { line: l, fault } if l == line => Some(fault),
        _ => None,
    }
}

// spec TRACE-6.1: each framing fault is named: no final line feed, no header, a blank line,
// a line that is not an object, and a line that is not JSON
#[test]
fn json_framing_faults() {
    let json = to_json(&sample());
    assert_eq!(
        fault_at(from_json(json.trim_end_matches('\n')).unwrap_err(), 12),
        Some(JsonFault::NoFinalLineFeed)
    );
    assert_eq!(
        fault_at(from_json("\n").unwrap_err(), 1),
        Some(JsonFault::BlankLine)
    );
    let blank = json_with(&["", "{\"t\":\"exit\",\"globals\":[],\"fields\":[]}"]);
    assert_eq!(
        fault_at(from_json(&blank).unwrap_err(), 2),
        Some(JsonFault::BlankLine)
    );
    assert_eq!(
        fault_at(json_line_error(&["[1]"]), 2),
        Some(JsonFault::NotAnObject)
    );
    assert!(matches!(
        fault_at(json_line_error(&["{"]), 2),
        Some(JsonFault::InvalidJson(_))
    ));
}

// spec TRACE-6.6: a reader rejects a missing key, an unknown key, a wrong JSON type and a
// number out of range, each named by its key
// spec TRACE-6.3: an unknown record name, and changes that are not arrays of pairs or triples
// spec TRACE-6.4: bits that are not 8 lowercase hex digits
// spec TRACE-6.5: text that is not escaped as TRACE-6.5 says
// spec TRACE-6.2: a header whose `trace` is not qc-trace
#[test]
fn json_record_faults() {
    let changes = "\"globals\":[],\"fields\":[]";
    let cases: [(String, JsonFault); 18] = [
        (format!("{{{changes}}}"), JsonFault::MissingKey("t")),
        (
            format!("{{\"t\":\"exit\",\"x\":1,{changes}}}"),
            JsonFault::UnknownKey("x".to_owned()),
        ),
        (format!("{{\"t\":5,{changes}}}"), JsonFault::NotAString("t")),
        (
            format!("{{\"t\":\"step\",\"function\":1,\"statement\":2,\"opcode\":-1,{changes}}}"),
            JsonFault::NotAU32("opcode"),
        ),
        (
            format!(
                "{{\"t\":\"error\",\"function\":1,\"statement\":3000000000,\"message\":\"\",{changes}}}"
            ),
            JsonFault::NotAnI32("statement"),
        ),
        (
            "{\"t\":\"string\",\"value\":\"ABC\",\"text\":\"\"}".to_owned(),
            JsonFault::NotBits("value"),
        ),
        (
            "{\"t\":\"string\",\"value\":\"00000001\",\"text\":\"\\\\x\"}".to_owned(),
            JsonFault::NotEscapedText("text"),
        ),
        (
            format!("{{\"t\":\"jump\",{changes}}}"),
            JsonFault::UnknownRecordType("jump".to_owned()),
        ),
        (
            "{\"t\":\"exit\",\"globals\":5,\"fields\":[]}".to_owned(),
            JsonFault::NotAnArray("globals"),
        ),
        (
            "{\"t\":\"exit\",\"globals\":[],\"fields\":5}".to_owned(),
            JsonFault::NotAnArray("fields"),
        ),
        (
            "{\"t\":\"exit\",\"globals\":[[1]],\"fields\":[]}".to_owned(),
            JsonFault::GlobalNotAPair,
        ),
        (
            "{\"t\":\"exit\",\"globals\":[[-1,\"00000000\"]],\"fields\":[]}".to_owned(),
            JsonFault::GlobalSlotOutOfRange,
        ),
        (
            "{\"t\":\"exit\",\"globals\":[[1,\"0\"]],\"fields\":[]}".to_owned(),
            JsonFault::GlobalBitsMalformed,
        ),
        (
            "{\"t\":\"exit\",\"globals\":[],\"fields\":[[1,2]]}".to_owned(),
            JsonFault::FieldNotATriple,
        ),
        (
            "{\"t\":\"exit\",\"globals\":[],\"fields\":[[-1,2,\"00000000\"]]}".to_owned(),
            JsonFault::EdictOutOfRange,
        ),
        (
            "{\"t\":\"exit\",\"globals\":[],\"fields\":[[1,-2,\"00000000\"]]}".to_owned(),
            JsonFault::FieldSlotOutOfRange,
        ),
        (
            "{\"t\":\"exit\",\"globals\":[],\"fields\":[[1,2,\"zz\"]]}".to_owned(),
            JsonFault::FieldBitsMalformed,
        ),
        (
            format!("{{\"t\":\"run\",{changes}}}"),
            JsonFault::MissingKey("function"),
        ),
    ];
    for (line, fault) in cases {
        assert_eq!(
            fault_at(json_line_error(&[&line]), 2),
            Some(fault),
            "{line}"
        );
    }
    let other = header_to_json(&header(HeaderSpec::default())).replace("qc-trace", "qc-trick");
    assert_eq!(
        fault_at(from_json(&other).unwrap_err(), 1),
        Some(JsonFault::NotQcTrace)
    );
}

// spec TRACE-6.6: each fault's message, byte for byte
#[test]
fn json_fault_messages() {
    let cases = [
        (JsonFault::NoFinalLineFeed, "the last line has no line feed"),
        (JsonFault::MissingHeader, "missing header"),
        (JsonFault::BlankLine, "blank line"),
        (JsonFault::NotAnObject, "not a JSON object"),
        (
            JsonFault::InvalidJson("eof".to_owned()),
            "invalid JSON: eof",
        ),
        (JsonFault::MissingKey("t"), "missing key `t`"),
        (JsonFault::UnknownKey("x".to_owned()), "unknown key `x`"),
        (JsonFault::NotAString("t"), "`t` is not a string"),
        (
            JsonFault::NotAU32("function"),
            "`function` is not an integer from 0 to 4294967295",
        ),
        (
            JsonFault::NotAnI32("statement"),
            "`statement` is not a 32-bit signed integer",
        ),
        (
            JsonFault::NotBits("value"),
            "`value` is not 8 lowercase hex digits",
        ),
        (
            JsonFault::NotEscapedText("text"),
            "`text` is not escaped text",
        ),
        (JsonFault::NotQcTrace, "`trace` is not \"qc-trace\""),
        (
            JsonFault::UnknownRecordType("jump".to_owned()),
            "unknown record type `jump`",
        ),
        (
            JsonFault::NotAnArray("globals"),
            "`globals` is not an array",
        ),
        (
            JsonFault::GlobalNotAPair,
            "a global change is not [slot, bits]",
        ),
        (
            JsonFault::GlobalSlotOutOfRange,
            "a global slot is out of range",
        ),
        (
            JsonFault::GlobalBitsMalformed,
            "a global's bits are malformed",
        ),
        (
            JsonFault::FieldNotATriple,
            "a field change is not [edict, field, bits]",
        ),
        (
            JsonFault::EdictOutOfRange,
            "an edict number is out of range",
        ),
        (
            JsonFault::FieldSlotOutOfRange,
            "a field slot is out of range",
        ),
        (
            JsonFault::FieldBitsMalformed,
            "a field's bits are malformed",
        ),
    ];
    for (fault, text) in cases {
        assert_eq!(fault.to_string(), text);
    }
    assert_eq!(
        TraceError::Json {
            line: 3,
            fault: JsonFault::BlankLine
        }
        .to_string(),
        "JSON trace line 3: blank line"
    );
}

// spec TRACE-6.2: the header line names the format and holds the version and five numbers
#[test]
fn json_header_line() {
    assert_eq!(
        header_to_json(&header(HeaderSpec::default())),
        "{\"trace\":\"qc-trace\",\"version\":1,\"progs_crc\":12345,\"edict_size\":516,\"field_offset\":96,\"entityfields\":105,\"numstrings\":16}\n"
    );
    let other = header_to_json(&header(HeaderSpec::default())).replace("qc-trace", "qc-trick");
    assert!(is_json_error(&from_json(&other).unwrap_err(), 1));
}

// spec TRACE-6.3: records are named by `t`, with `builtin` only when there is one
#[test]
fn json_record_lines() {
    assert_eq!(
        record_to_json(&step(1, 2, 3, None, Changes::default())),
        "{\"t\":\"step\",\"function\":1,\"statement\":2,\"opcode\":3,\"globals\":[],\"fields\":[]}\n"
    );
    assert_eq!(
        record_to_json(&step(1, 2, 51, Some(0), Changes::default())),
        "{\"t\":\"step\",\"function\":1,\"statement\":2,\"opcode\":51,\"builtin\":0,\"globals\":[],\"fields\":[]}\n"
    );
    let err = json_line_error(&["{\"t\":\"jump\",\"globals\":[],\"fields\":[]}"]);
    assert!(is_json_error(&err, 2));
    let decoded = from_json(&json_with(&["{\"t\":\"step\",\"function\":1,\"statement\":2,\"opcode\":51,\"builtin\":0,\"globals\":[],\"fields\":[]}"])).unwrap();
    assert_eq!(
        decoded.records,
        [step(1, 2, 51, Some(0), Changes::default())]
    );
}

// spec TRACE-6.4: bits are exactly 8 lowercase hex digits
#[test]
fn json_bits() {
    let line = record_to_json(&Record::String {
        value: 0xabc,
        text: text(b""),
    });
    assert_eq!(
        line,
        "{\"t\":\"string\",\"value\":\"00000abc\",\"text\":\"\"}\n"
    );
    for bad in [
        "\"00000ABC\"",
        "\"abc\"",
        "\"000000abc\"",
        "2748",
        "\"0000 abc\"",
        "\"+0000abc\"",
    ] {
        let err = json_line_error(&[&format!(
            "{{\"t\":\"string\",\"value\":{bad},\"text\":\"\"}}"
        )]);
        assert!(is_json_error(&err, 2), "{bad}");
    }
    let err = json_line_error(&["{\"t\":\"exit\",\"globals\":[[1,\"3F800000\"]],\"fields\":[]}"]);
    assert!(is_json_error(&err, 2));
    let err = json_line_error(&["{\"t\":\"exit\",\"globals\":[],\"fields\":[[1,2,\"3F800000\"]]}"]);
    assert!(is_json_error(&err, 2));
}

// spec TRACE-6.5: printable bytes but the backslash stand for themselves, the rest are \xNN
#[test]
fn json_text_escapes() {
    let all: Vec<u8> = (1..=255).collect();
    let line = record_to_json(&Record::String {
        value: 0,
        text: text(&all),
    });
    assert!(line.contains("\\\\x01"));
    assert!(line.contains("\\\\x1f !\\\"#"));
    assert!(line.contains("[\\\\x5c]"));
    assert!(line.contains("}~\\\\x7f\\\\x80"));
    assert!(line.contains("\\\\xff\""));
    assert_eq!(
        from_json(&json_with(&[line.trim_end()])).unwrap().records,
        [Record::String {
            value: 0,
            text: text(&all)
        }]
    );
    assert!(
        record_to_json(&Record::String {
            value: 0,
            text: text(b"a\\b\n")
        })
        .contains("\"a\\\\x5cb\\\\x0a\"")
    );
    for bad in [
        "\\\\x41", "\\\\x0A", "\\\\x5", "\\\\y00", "\\\\", "\\n", "\\u00e9", "\\t",
    ] {
        let err = json_line_error(&[&format!(
            "{{\"t\":\"string\",\"value\":\"00000000\",\"text\":\"{bad}\"}}"
        )]);
        assert!(is_json_error(&err, 2), "{bad}");
    }
}

// spec TRACE-6.6: a writer is canonical; a reader takes any key order and whitespace
#[test]
fn json_reader_is_lenient_about_layout() {
    let line =
        " { \"fields\" : [ ] , \"globals\" : [ [ 7 , \"00000001\" ] ] ,\t\"t\" : \"exit\" } ";
    let trace = from_json(&json_with(&[line])).unwrap();
    assert_eq!(
        trace.records,
        [Record::Exit {
            changes: globals(&[(7, 1)])
        }]
    );
    assert_eq!(
        record_to_json(&trace.records[0]),
        "{\"t\":\"exit\",\"globals\":[[7,\"00000001\"]],\"fields\":[]}\n"
    );
}

// spec TRACE-6.6: a reader rejects missing and unknown keys, wrong types and out-of-range numbers
#[test]
fn json_reader_rejects_bad_records() {
    let bad_lines = [
        "{\"t\":\"exit\",\"globals\":[]}",
        "{\"t\":\"exit\",\"globals\":[],\"fields\":[],\"extra\":1}",
        "{\"globals\":[],\"fields\":[]}",
        "{\"t\":1,\"globals\":[],\"fields\":[]}",
        "{\"t\":\"exit\",\"globals\":{},\"fields\":[]}",
        "{\"t\":\"exit\",\"globals\":[],\"fields\":{}}",
        "{\"t\":\"exit\",\"globals\":[[1]],\"fields\":[]}",
        "{\"t\":\"exit\",\"globals\":[[-1,\"00000000\"]],\"fields\":[]}",
        "{\"t\":\"exit\",\"globals\":[[4294967296,\"00000000\"]],\"fields\":[]}",
        "{\"t\":\"exit\",\"globals\":[],\"fields\":[[1,2]]}",
        "{\"t\":\"exit\",\"globals\":[],\"fields\":[[-1,2,\"00000000\"]]}",
        "{\"t\":\"exit\",\"globals\":[],\"fields\":[[1,-2,\"00000000\"]]}",
        "{\"t\":\"run\",\"function\":2147483648,\"globals\":[],\"fields\":[]}",
        "{\"t\":\"run\",\"function\":1.0,\"globals\":[],\"fields\":[]}",
        "{\"t\":\"run\",\"function\":\"1\",\"globals\":[],\"fields\":[]}",
        "{\"t\":\"step\",\"function\":1,\"statement\":1,\"globals\":[],\"fields\":[]}",
        "{\"t\":\"step\",\"function\":1,\"statement\":1,\"opcode\":0,\"builtin\":-1,\"globals\":[],\"fields\":[]}",
        "{\"t\":\"error\",\"function\":1,\"statement\":1,\"message\":7,\"globals\":[],\"fields\":[]}",
        "{\"t\":\"string\",\"value\":\"00000000\"}",
    ];
    for line in bad_lines {
        assert!(is_json_error(&json_line_error(&[line]), 2), "{line}");
    }
    let opcode = json_line_error(&[
        "{\"t\":\"step\",\"function\":1,\"statement\":1,\"opcode\":65536,\"globals\":[],\"fields\":[]}",
    ]);
    assert_eq!(opcode, TraceError::OpcodeOutOfRange(65536));
    let line = header_to_json(&header(HeaderSpec::default()));
    let missing = line.replace(",\"numstrings\":16", "");
    assert!(is_json_error(&from_json(&missing).unwrap_err(), 1));
    let unknown = line.replace("\"numstrings\":16", "\"numstrings\":16,\"x\":0");
    assert!(is_json_error(&from_json(&unknown).unwrap_err(), 1));
}

// ---------------------------------------------------------------------------------------
// TRACE-7

// spec TRACE-7.1: an entity value is n * edict_size exactly, n 0 or more
#[test]
fn entity_numbers() {
    let h = header(HeaderSpec::default());
    assert_eq!(h.entity_number(0), Some(0));
    assert_eq!(h.entity_number(516), Some(1));
    assert_eq!(h.entity_number(516 * 599), Some(599));
    assert_eq!(h.entity_number(515), None);
    assert_eq!(h.entity_number(517), None);
    assert_eq!((-516i32).cast_unsigned(), 0xffff_fdfc);
    assert_eq!(h.entity_number(0xffff_fdfc), None);
    assert_eq!(h.entity_number(0x3f80_0000), None);
    let one = header(HeaderSpec {
        edict_size: 1,
        field_offset: 0,
        entityfields: 0,
        ..HeaderSpec::default()
    });
    assert_eq!(one.entity_number(0x7fff_ffff), Some(0x7fff_ffff));
    assert_eq!(one.entity_number(0x8000_0000), None);
}

// spec TRACE-7.2: a pointer is n * edict_size + field_offset + 4 * f, f below entityfields
#[test]
fn pointer_targets() {
    let h = header(HeaderSpec::default());
    assert_eq!(h.pointer_target(96), Some((0, 0)));
    assert_eq!(h.pointer_target(516 + 96 + 4 * 30), Some((1, 30)));
    assert_eq!(h.pointer_target(516 * 2 + 96 + 4 * 104), Some((2, 104)));
    assert_eq!(h.pointer_target(516 + 96 + 4 * 30 + 1), None);
    assert_eq!(h.pointer_target(516 + 95), None);
    assert_eq!(h.pointer_target(0), None);
    assert_eq!(h.pointer_target((-420i32).cast_unsigned()), None);
    let packed = header(HeaderSpec {
        edict_size: 8,
        field_offset: 0,
        entityfields: 2,
        ..HeaderSpec::default()
    });
    assert_eq!(packed.pointer_target(0), Some((0, 0)));
    assert_eq!(packed.pointer_target(12), Some((1, 1)));
    let gap = header(HeaderSpec {
        edict_size: 600,
        ..HeaderSpec::default()
    });
    assert_eq!(gap.pointer_target(96 + 4 * 104), Some((0, 104)));
    assert_eq!(gap.pointer_target(96 + 4 * 105), None);
}

// spec TRACE-7.3: a table offset is itself; any other value names the latest string record before the point
#[test]
fn string_values() {
    let trace = Trace {
        header: header(HeaderSpec::default()),
        records: vec![
            Record::String {
                value: 100,
                text: text(b"one"),
            },
            Record::Exit {
                changes: Changes::default(),
            },
            Record::String {
                value: 100,
                text: text(b"two"),
            },
            Record::String {
                value: 200,
                text: text(b"other"),
            },
        ],
    };
    assert_eq!(trace.string_value(0, 0), StringValue::Table(0));
    assert_eq!(trace.string_value(0, 15), StringValue::Table(15));
    assert_eq!(trace.string_value(0, 100), StringValue::Unknown);
    assert_eq!(
        trace.string_value(1, 100),
        StringValue::Engine(&text(b"one"))
    );
    assert_eq!(
        trace.string_value(2, 100),
        StringValue::Engine(&text(b"one"))
    );
    assert_eq!(
        trace.string_value(3, 100),
        StringValue::Engine(&text(b"two"))
    );
    assert_eq!(
        trace.string_value(99, 100),
        StringValue::Engine(&text(b"two"))
    );
    assert_eq!(trace.string_value(99, 16), StringValue::Unknown);
    assert_eq!(
        trace.string_value(99, 200),
        StringValue::Engine(&text(b"other"))
    );
    assert_eq!(trace.string_value(99, 0xffff_ffff), StringValue::Unknown);
}

// No spec statement: TRACE-5.7 says what a reader rejects, not how the error reads; this
// pins that each error's text is distinct and carries its offset, tag or line.
#[test]
fn errors_display() {
    let errors = [
        TraceError::BadHeader,
        TraceError::UnsupportedVersion(2),
        TraceError::BadMagic,
        TraceError::Truncated { offset: 3 },
        TraceError::BadUvar { offset: 4 },
        TraceError::UnknownTag { offset: 5, tag: 9 },
        TraceError::BadBuiltinFlag { offset: 6 },
        TraceError::OpcodeOutOfRange(70_000),
        TraceError::DuplicateChange,
        TraceError::ChangesOutOfOrder,
        TraceError::NulInText,
        TraceError::TooLong,
        TraceError::Json {
            line: 7,
            fault: JsonFault::BlankLine,
        },
    ];
    let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
    for (i, a) in messages.iter().enumerate() {
        assert!(!a.is_empty());
        for b in &messages[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert!(messages[12].contains("line 7") && messages[12].contains("blank line"));
    assert!(messages[5].contains("0x09") && messages[5].contains('5'));
}

// ---------------------------------------------------------------------------------------
// Properties

fn any_changes() -> impl Strategy<Value = Changes> {
    let global = (0u32..2000, any::<u32>()).prop_map(|(slot, bits)| GlobalChange { slot, bits });
    let field = (0u32..64, 0u32..200, any::<u32>()).prop_map(|(edict, field, bits)| FieldChange {
        edict,
        field,
        bits,
    });
    (
        proptest::collection::vec(global, 0..6),
        proptest::collection::vec(field, 0..6),
    )
        .prop_map(|(mut g, mut f)| {
            g.sort_by_key(|c| c.slot);
            g.dedup_by_key(|c| c.slot);
            f.sort_by_key(|c| (c.edict, c.field));
            f.dedup_by_key(|c| (c.edict, c.field));
            Changes::new(g, f).unwrap()
        })
}

fn any_text() -> impl Strategy<Value = Text> {
    proptest::collection::vec(1u8..=255, 0..40).prop_map(|b| Text::new(b).unwrap())
}

fn any_record() -> impl Strategy<Value = Record> {
    prop_oneof![
        (any::<i32>(), any_changes())
            .prop_map(|(function, changes)| Record::Run { function, changes }),
        (
            any::<i32>(),
            any::<i32>(),
            any::<u16>(),
            proptest::option::of(any::<u32>()),
            any_changes()
        )
            .prop_map(|(f, s, o, b, c)| step(f, s, o, b, c)),
        any_changes().prop_map(|changes| Record::Exit { changes }),
        (any::<i32>(), any::<i32>(), any_text(), any_changes()).prop_map(
            |(function, statement, message, changes)| {
                Record::Error {
                    function,
                    statement,
                    message,
                    changes,
                }
            }
        ),
        (any::<u32>(), any_text()).prop_map(|(value, text)| Record::String { value, text }),
    ]
}

fn any_header() -> impl Strategy<Value = Header> {
    (
        any::<u16>(),
        1u32..100_000,
        0u32..1000,
        0u32..1000,
        any::<u32>(),
    )
        .prop_map(|(crc, size, offset, count, strings)| {
            let size = size.max(offset + 4 * count);
            Header::new(crc, size, offset, count, strings).unwrap()
        })
}

fn any_trace() -> impl Strategy<Value = Trace> {
    (any_header(), proptest::collection::vec(any_record(), 0..12))
        .prop_map(|(header, records)| Trace { header, records })
}

proptest! {
    // spec TRACE-1.2, TRACE-5.7: binary decode undoes encode
    #[test]
    fn binary_round_trip(trace in any_trace()) {
        prop_assert_eq!(decode(&encode(&trace)).unwrap(), trace);
    }

    // spec TRACE-1.2, TRACE-6.6: JSON decode undoes encode
    #[test]
    fn json_round_trip(trace in any_trace()) {
        prop_assert_eq!(from_json(&to_json(&trace)).unwrap(), trace);
    }

    // spec TRACE-1.2: binary to JSON to binary loses nothing
    #[test]
    fn forms_convert_losslessly(trace in any_trace()) {
        let bytes = encode(&trace);
        let via_json = from_json(&to_json(&decode(&bytes).unwrap())).unwrap();
        prop_assert_eq!(encode(&via_json), bytes);
    }

    // spec TRACE-5.7: every accepted stream has one encoding, so re-encoding gives the same bytes
    #[test]
    fn accepted_streams_are_canonical(trace in any_trace(), index in any::<proptest::sample::Index>(), byte in any::<u8>(), cut in any::<proptest::sample::Index>()) {
        let mut bytes = encode(&trace);
        let at = index.index(bytes.len());
        bytes[at] = byte;
        let end = TRACE_HEADER_SIZE + cut.index(bytes.len() - TRACE_HEADER_SIZE + 1);
        bytes.truncate(end);
        if let Ok(decoded) = decode(&bytes) {
            prop_assert_eq!(encode(&decoded), bytes);
        }
    }
}

// spec TRACE-7.3: engine strings take negative values (EDICT-13.3); after a binary round trip
// the bits of -1 and -2 each name the latest string record for them before the point read, the
// temp buffer's -1 changing its text between records, and a value with no record names none
#[test]
fn reviewer_negative_engine_string_values_resolve() {
    let bits = |value: i32| value.cast_unsigned();
    let trace = Trace {
        header: header(HeaderSpec::default()),
        records: vec![
            Record::String {
                value: bits(-1),
                text: text(b"0"),
            },
            Record::String {
                value: bits(-2),
                text: text(b"start"),
            },
            Record::Exit {
                changes: Changes::default(),
            },
            Record::String {
                value: bits(-1),
                text: text(b"  0.5"),
            },
        ],
    };
    let decoded = decode(&encode(&trace)).unwrap();
    assert_eq!(decoded, trace);
    assert_eq!(
        decoded.string_value(3, bits(-1)),
        StringValue::Engine(&text(b"0"))
    );
    assert_eq!(
        decoded.string_value(4, bits(-1)),
        StringValue::Engine(&text(b"  0.5"))
    );
    assert_eq!(
        decoded.string_value(4, bits(-2)),
        StringValue::Engine(&text(b"start"))
    );
    assert_eq!(decoded.string_value(4, bits(-3)), StringValue::Unknown);
}
