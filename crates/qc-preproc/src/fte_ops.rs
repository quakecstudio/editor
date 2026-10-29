// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, pr_opcodes

//! FTEQCC's opcodes (FTEF-23.1): one [`Op`] for each row of `pr_opcodes[]`, named after its
//! `OP_*` constant in `pr_comp.h`'s `qcop_e`, whose number it is. The row's `opname`, which
//! `#pragma opcode` and FTEQCC's messages use, differs from the constant for 24 of them
//! (`OP_CONV_ITOF` is `CONV_IF`); two rows share the `opname` `BITCLR_I`.

/// An opcode of FTEQCC's `pr_opcodes[]`; its discriminant is its `OP_*` number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum Op {
    /// `OP_DONE`.
    Done,
    /// `OP_MUL_F`.
    MulF,
    /// `OP_MUL_V`.
    MulV,
    /// `OP_MUL_FV`.
    MulFv,
    /// `OP_MUL_VF`.
    MulVf,
    /// `OP_DIV_F`.
    DivF,
    /// `OP_ADD_F`.
    AddF,
    /// `OP_ADD_V`.
    AddV,
    /// `OP_SUB_F`.
    SubF,
    /// `OP_SUB_V`.
    SubV,
    /// `OP_EQ_F`.
    EqF,
    /// `OP_EQ_V`.
    EqV,
    /// `OP_EQ_S`.
    EqS,
    /// `OP_EQ_E`.
    EqE,
    /// `OP_EQ_FNC`.
    EqFnc,
    /// `OP_NE_F`.
    NeF,
    /// `OP_NE_V`.
    NeV,
    /// `OP_NE_S`.
    NeS,
    /// `OP_NE_E`.
    NeE,
    /// `OP_NE_FNC`.
    NeFnc,
    /// `OP_LE_F`.
    LeF,
    /// `OP_GE_F`.
    GeF,
    /// `OP_LT_F`.
    LtF,
    /// `OP_GT_F`.
    GtF,
    /// `OP_LOAD_F`, `opname` `LOADF_F`.
    LoadF,
    /// `OP_LOAD_V`, `opname` `LOADF_V`.
    LoadV,
    /// `OP_LOAD_S`, `opname` `LOADF_S`.
    LoadS,
    /// `OP_LOAD_ENT`, `opname` `LOADF_ENT`.
    LoadEnt,
    /// `OP_LOAD_FLD`, `opname` `LOADF_FLD`.
    LoadFld,
    /// `OP_LOAD_FNC`, `opname` `LOADF_FNC`.
    LoadFnc,
    /// `OP_ADDRESS`, `opname` `FLDADDRESS`.
    Address,
    /// `OP_STORE_F`.
    StoreF,
    /// `OP_STORE_V`.
    StoreV,
    /// `OP_STORE_S`.
    StoreS,
    /// `OP_STORE_ENT`.
    StoreEnt,
    /// `OP_STORE_FLD`.
    StoreFld,
    /// `OP_STORE_FNC`.
    StoreFnc,
    /// `OP_STOREP_F`.
    StorepF,
    /// `OP_STOREP_V`.
    StorepV,
    /// `OP_STOREP_S`.
    StorepS,
    /// `OP_STOREP_ENT`.
    StorepEnt,
    /// `OP_STOREP_FLD`.
    StorepFld,
    /// `OP_STOREP_FNC`.
    StorepFnc,
    /// `OP_RETURN`.
    Return,
    /// `OP_NOT_F`.
    NotF,
    /// `OP_NOT_V`.
    NotV,
    /// `OP_NOT_S`.
    NotS,
    /// `OP_NOT_ENT`.
    NotEnt,
    /// `OP_NOT_FNC`.
    NotFnc,
    /// `OP_IF_I`, `opname` `IF`.
    IfI,
    /// `OP_IFNOT_I`, `opname` `IFNOT`.
    IfnotI,
    /// `OP_CALL0`.
    Call0,
    /// `OP_CALL1`.
    Call1,
    /// `OP_CALL2`.
    Call2,
    /// `OP_CALL3`.
    Call3,
    /// `OP_CALL4`.
    Call4,
    /// `OP_CALL5`.
    Call5,
    /// `OP_CALL6`.
    Call6,
    /// `OP_CALL7`.
    Call7,
    /// `OP_CALL8`.
    Call8,
    /// `OP_STATE`.
    State,
    /// `OP_GOTO`.
    Goto,
    /// `OP_AND_F`.
    AndF,
    /// `OP_OR_F`.
    OrF,
    /// `OP_BITAND_F`, `opname` `BITAND`.
    BitandF,
    /// `OP_BITOR_F`, `opname` `BITOR`.
    BitorF,
    /// `OP_MULSTORE_F`.
    MulstoreF,
    /// `OP_MULSTORE_VF`.
    MulstoreVf,
    /// `OP_MULSTOREP_F`.
    MulstorepF,
    /// `OP_MULSTOREP_VF`.
    MulstorepVf,
    /// `OP_DIVSTORE_F`.
    DivstoreF,
    /// `OP_DIVSTOREP_F`.
    DivstorepF,
    /// `OP_ADDSTORE_F`.
    AddstoreF,
    /// `OP_ADDSTORE_V`.
    AddstoreV,
    /// `OP_ADDSTOREP_F`.
    AddstorepF,
    /// `OP_ADDSTOREP_V`.
    AddstorepV,
    /// `OP_SUBSTORE_F`.
    SubstoreF,
    /// `OP_SUBSTORE_V`.
    SubstoreV,
    /// `OP_SUBSTOREP_F`.
    SubstorepF,
    /// `OP_SUBSTOREP_V`.
    SubstorepV,
    /// `OP_FETCH_GBL_F`.
    FetchGblF,
    /// `OP_FETCH_GBL_V`.
    FetchGblV,
    /// `OP_FETCH_GBL_S`.
    FetchGblS,
    /// `OP_FETCH_GBL_E`.
    FetchGblE,
    /// `OP_FETCH_GBL_FNC`.
    FetchGblFnc,
    /// `OP_CSTATE`.
    Cstate,
    /// `OP_CWSTATE`.
    Cwstate,
    /// `OP_THINKTIME`.
    Thinktime,
    /// `OP_BITSETSTORE_F`.
    BitsetstoreF,
    /// `OP_BITSETSTOREP_F`.
    BitsetstorepF,
    /// `OP_BITCLRSTORE_F`.
    BitclrstoreF,
    /// `OP_BITCLRSTOREP_F`.
    BitclrstorepF,
    /// `OP_RAND0`.
    Rand0,
    /// `OP_RAND1`.
    Rand1,
    /// `OP_RAND2`.
    Rand2,
    /// `OP_RANDV0`.
    Randv0,
    /// `OP_RANDV1`.
    Randv1,
    /// `OP_RANDV2`.
    Randv2,
    /// `OP_SWITCH_F`.
    SwitchF,
    /// `OP_SWITCH_V`.
    SwitchV,
    /// `OP_SWITCH_S`.
    SwitchS,
    /// `OP_SWITCH_E`.
    SwitchE,
    /// `OP_SWITCH_FNC`.
    SwitchFnc,
    /// `OP_CASE`.
    Case,
    /// `OP_CASERANGE`.
    Caserange,
    /// `OP_CALL1H`.
    Call1h,
    /// `OP_CALL2H`.
    Call2h,
    /// `OP_CALL3H`.
    Call3h,
    /// `OP_CALL4H`.
    Call4h,
    /// `OP_CALL5H`.
    Call5h,
    /// `OP_CALL6H`.
    Call6h,
    /// `OP_CALL7H`.
    Call7h,
    /// `OP_CALL8H`.
    Call8h,
    /// `OP_STORE_I`.
    StoreI,
    /// `OP_STORE_IF`.
    StoreIf,
    /// `OP_STORE_FI`.
    StoreFi,
    /// `OP_ADD_I`.
    AddI,
    /// `OP_ADD_FI`.
    AddFi,
    /// `OP_ADD_IF`.
    AddIf,
    /// `OP_SUB_I`.
    SubI,
    /// `OP_SUB_FI`.
    SubFi,
    /// `OP_SUB_IF`.
    SubIf,
    /// `OP_CONV_ITOF`, `opname` `CONV_IF`.
    ConvItof,
    /// `OP_CONV_FTOI`, `opname` `CONV_FI`.
    ConvFtoi,
    /// `OP_LOADP_ITOF`, `opname` `CONVP_IF`.
    LoadpItof,
    /// `OP_LOADP_FTOI`, `opname` `CONVP_FI`.
    LoadpFtoi,
    /// `OP_LOAD_I`, `opname` `LOADF_I`.
    LoadI,
    /// `OP_STOREP_I`.
    StorepI,
    /// `OP_STOREP_IF`.
    StorepIf,
    /// `OP_STOREP_FI`.
    StorepFi,
    /// `OP_BITAND_I`.
    BitandI,
    /// `OP_BITOR_I`.
    BitorI,
    /// `OP_MUL_I`.
    MulI,
    /// `OP_DIV_I`.
    DivI,
    /// `OP_EQ_I`.
    EqI,
    /// `OP_NE_I`.
    NeI,
    /// `OP_IFNOT_S`, `opname` `IFNOTS`.
    IfnotS,
    /// `OP_IF_S`, `opname` `IFS`.
    IfS,
    /// `OP_NOT_I`.
    NotI,
    /// `OP_DIV_VF`.
    DivVf,
    /// `OP_BITXOR_I`.
    BitxorI,
    /// `OP_RSHIFT_I`.
    RshiftI,
    /// `OP_LSHIFT_I`.
    LshiftI,
    /// `OP_GLOBALADDRESS`.
    Globaladdress,
    /// `OP_ADD_PIW`.
    AddPiw,
    /// `OP_LOADA_F`.
    LoadaF,
    /// `OP_LOADA_V`.
    LoadaV,
    /// `OP_LOADA_S`.
    LoadaS,
    /// `OP_LOADA_ENT`.
    LoadaEnt,
    /// `OP_LOADA_FLD`.
    LoadaFld,
    /// `OP_LOADA_FNC`.
    LoadaFnc,
    /// `OP_LOADA_I`.
    LoadaI,
    /// `OP_STORE_P`.
    StoreP,
    /// `OP_LOAD_P`, `opname` `LOADF_P`.
    LoadP,
    /// `OP_LOADP_F`.
    LoadpF,
    /// `OP_LOADP_V`.
    LoadpV,
    /// `OP_LOADP_S`.
    LoadpS,
    /// `OP_LOADP_ENT`.
    LoadpEnt,
    /// `OP_LOADP_FLD`.
    LoadpFld,
    /// `OP_LOADP_FNC`.
    LoadpFnc,
    /// `OP_LOADP_I`.
    LoadpI,
    /// `OP_LE_I`.
    LeI,
    /// `OP_GE_I`.
    GeI,
    /// `OP_LT_I`.
    LtI,
    /// `OP_GT_I`.
    GtI,
    /// `OP_LE_IF`.
    LeIf,
    /// `OP_GE_IF`.
    GeIf,
    /// `OP_LT_IF`.
    LtIf,
    /// `OP_GT_IF`.
    GtIf,
    /// `OP_LE_FI`.
    LeFi,
    /// `OP_GE_FI`.
    GeFi,
    /// `OP_LT_FI`.
    LtFi,
    /// `OP_GT_FI`.
    GtFi,
    /// `OP_EQ_IF`.
    EqIf,
    /// `OP_EQ_FI`.
    EqFi,
    /// `OP_ADD_SF`.
    AddSf,
    /// `OP_SUB_S`.
    SubS,
    /// `OP_STOREP_C`.
    StorepC,
    /// `OP_LOADP_C`.
    LoadpC,
    /// `OP_MUL_IF`.
    MulIf,
    /// `OP_MUL_FI`.
    MulFi,
    /// `OP_MUL_VI`.
    MulVi,
    /// `OP_MUL_IV`.
    MulIv,
    /// `OP_DIV_IF`.
    DivIf,
    /// `OP_DIV_FI`.
    DivFi,
    /// `OP_BITAND_IF`.
    BitandIf,
    /// `OP_BITOR_IF`.
    BitorIf,
    /// `OP_BITAND_FI`.
    BitandFi,
    /// `OP_BITOR_FI`.
    BitorFi,
    /// `OP_AND_I`.
    AndI,
    /// `OP_OR_I`.
    OrI,
    /// `OP_AND_IF`.
    AndIf,
    /// `OP_OR_IF`.
    OrIf,
    /// `OP_AND_FI`.
    AndFi,
    /// `OP_OR_FI`.
    OrFi,
    /// `OP_NE_IF`.
    NeIf,
    /// `OP_NE_FI`.
    NeFi,
    /// `OP_GSTOREP_I`.
    GstorepI,
    /// `OP_GSTOREP_F`.
    GstorepF,
    /// `OP_GSTOREP_ENT`.
    GstorepEnt,
    /// `OP_GSTOREP_FLD`.
    GstorepFld,
    /// `OP_GSTOREP_S`.
    GstorepS,
    /// `OP_GSTOREP_FNC`.
    GstorepFnc,
    /// `OP_GSTOREP_V`.
    GstorepV,
    /// `OP_GADDRESS`.
    Gaddress,
    /// `OP_GLOAD_I`.
    GloadI,
    /// `OP_GLOAD_F`.
    GloadF,
    /// `OP_GLOAD_FLD`.
    GloadFld,
    /// `OP_GLOAD_ENT`.
    GloadEnt,
    /// `OP_GLOAD_S`.
    GloadS,
    /// `OP_GLOAD_FNC`.
    GloadFnc,
    /// `OP_BOUNDCHECK`.
    Boundcheck,
    /// `OP_UNUSED`.
    Unused,
    /// `OP_PUSH`.
    Push,
    /// `OP_POP`.
    Pop,
    /// `OP_SWITCH_I`.
    SwitchI,
    /// `OP_GLOAD_V`.
    GloadV,
    /// `OP_IF_F`.
    IfF,
    /// `OP_IFNOT_F`.
    IfnotF,
    /// `OP_STOREF_V`.
    StorefV,
    /// `OP_STOREF_F`.
    StorefF,
    /// `OP_STOREF_S`.
    StorefS,
    /// `OP_STOREF_I`.
    StorefI,
    /// `OP_STOREP_I8`.
    StorepI8,
    /// `OP_LOADP_U8`.
    LoadpU8,
    /// `OP_LE_U`.
    LeU,
    /// `OP_LT_U`.
    LtU,
    /// `OP_DIV_U`.
    DivU,
    /// `OP_RSHIFT_U`.
    RshiftU,
    /// `OP_ADD_I64`.
    AddI64,
    /// `OP_SUB_I64`.
    SubI64,
    /// `OP_MUL_I64`.
    MulI64,
    /// `OP_DIV_I64`.
    DivI64,
    /// `OP_BITAND_I64`.
    BitandI64,
    /// `OP_BITOR_I64`.
    BitorI64,
    /// `OP_BITXOR_I64`.
    BitxorI64,
    /// `OP_LSHIFT_I64I`.
    LshiftI64i,
    /// `OP_RSHIFT_I64I`.
    RshiftI64i,
    /// `OP_LE_I64`.
    LeI64,
    /// `OP_LT_I64`.
    LtI64,
    /// `OP_EQ_I64`.
    EqI64,
    /// `OP_NE_I64`.
    NeI64,
    /// `OP_LE_U64`.
    LeU64,
    /// `OP_LT_U64`.
    LtU64,
    /// `OP_DIV_U64`.
    DivU64,
    /// `OP_RSHIFT_U64I`.
    RshiftU64i,
    /// `OP_STORE_I64`.
    StoreI64,
    /// `OP_STOREP_I64`.
    StorepI64,
    /// `OP_STOREF_I64`.
    StorefI64,
    /// `OP_LOAD_I64`, `opname` `LOADF_I64`.
    LoadI64,
    /// `OP_LOADA_I64`.
    LoadaI64,
    /// `OP_LOADP_I64`.
    LoadpI64,
    /// `OP_CONV_UI64`.
    ConvUi64,
    /// `OP_CONV_II64`.
    ConvIi64,
    /// `OP_CONV_I64I`.
    ConvI64i,
    /// `OP_CONV_FD`.
    ConvFd,
    /// `OP_CONV_DF`.
    ConvDf,
    /// `OP_CONV_I64F`.
    ConvI64f,
    /// `OP_CONV_FI64`.
    ConvFi64,
    /// `OP_CONV_I64D`.
    ConvI64d,
    /// `OP_CONV_DI64`.
    ConvDi64,
    /// `OP_ADD_D`.
    AddD,
    /// `OP_SUB_D`.
    SubD,
    /// `OP_MUL_D`.
    MulD,
    /// `OP_DIV_D`.
    DivD,
    /// `OP_LE_D`.
    LeD,
    /// `OP_LT_D`.
    LtD,
    /// `OP_EQ_D`.
    EqD,
    /// `OP_NE_D`.
    NeD,
    /// `OP_STOREP_I16`.
    StorepI16,
    /// `OP_LOADP_I16`.
    LoadpI16,
    /// `OP_LOADP_U16`.
    LoadpU16,
    /// `OP_LOADP_I8`.
    LoadpI8,
    /// `OP_BITEXTEND_I`.
    BitextendI,
    /// `OP_BITEXTEND_U`.
    BitextendU,
    /// `OP_BITCOPY_I`.
    BitcopyI,
    /// `OP_CONV_UF`.
    ConvUf,
    /// `OP_CONV_FU`.
    ConvFu,
    /// `OP_CONV_U64D`.
    ConvU64d,
    /// `OP_CONV_DU64`.
    ConvDu64,
    /// `OP_CONV_U64F`.
    ConvU64f,
    /// `OP_CONV_FU64`.
    ConvFu64,
    /// `OP_NUMREALOPS`, `opname` `OP_EMULATED`.
    Numrealops,
    /// `OP_BITSETSTORE_I`, `opname` `BITSET_I`.
    BitsetstoreI,
    /// `OP_BITSETSTOREP_I`, `opname` `BITSETP_I`.
    BitsetstorepI,
    /// `OP_BITCLRSTORE_I`, `opname` `BITCLR_I`.
    BitclrstoreI,
    /// `OP_MULSTORE_I`.
    MulstoreI,
    /// `OP_DIVSTORE_I`.
    DivstoreI,
    /// `OP_ADDSTORE_I`.
    AddstoreI,
    /// `OP_SUBSTORE_I`.
    SubstoreI,
    /// `OP_MULSTOREP_I`.
    MulstorepI,
    /// `OP_DIVSTOREP_I`.
    DivstorepI,
    /// `OP_ADDSTOREP_I`.
    AddstorepI,
    /// `OP_SUBSTOREP_I`.
    SubstorepI,
    /// `OP_MULSTORE_IF`.
    MulstoreIf,
    /// `OP_MULSTOREP_IF`.
    MulstorepIf,
    /// `OP_DIVSTORE_IF`.
    DivstoreIf,
    /// `OP_DIVSTOREP_IF`.
    DivstorepIf,
    /// `OP_ADDSTORE_IF`.
    AddstoreIf,
    /// `OP_ADDSTOREP_IF`.
    AddstorepIf,
    /// `OP_SUBSTORE_IF`.
    SubstoreIf,
    /// `OP_SUBSTOREP_IF`.
    SubstorepIf,
    /// `OP_MULSTORE_FI`.
    MulstoreFi,
    /// `OP_MULSTOREP_FI`.
    MulstorepFi,
    /// `OP_DIVSTORE_FI`.
    DivstoreFi,
    /// `OP_DIVSTOREP_FI`.
    DivstorepFi,
    /// `OP_ADDSTORE_FI`.
    AddstoreFi,
    /// `OP_ADDSTOREP_FI`.
    AddstorepFi,
    /// `OP_SUBSTORE_FI`.
    SubstoreFi,
    /// `OP_SUBSTOREP_FI`.
    SubstorepFi,
    /// `OP_MULSTORE_VI`.
    MulstoreVi,
    /// `OP_MULSTOREP_VI`.
    MulstorepVi,
    /// `OP_LOADA_STRUCT`.
    LoadaStruct,
    /// `OP_LOADP_P`.
    LoadpP,
    /// `OP_STOREP_P`.
    StorepP,
    /// `OP_BITNOT_F`.
    BitnotF,
    /// `OP_BITNOT_I`.
    BitnotI,
    /// `OP_EQ_P`.
    EqP,
    /// `OP_NE_P`.
    NeP,
    /// `OP_LE_P`.
    LeP,
    /// `OP_GE_P`.
    GeP,
    /// `OP_LT_P`.
    LtP,
    /// `OP_GT_P`.
    GtP,
    /// `OP_ANDSTORE_F`.
    AndstoreF,
    /// `OP_BITCLR_F`.
    BitclrF,
    /// `OP_BITCLR_I`.
    BitclrI,
    /// `OP_BITCLR_V`.
    BitclrV,
    /// `OP_ADD_SI`.
    AddSi,
    /// `OP_ADD_IS`.
    AddIs,
    /// `OP_ADD_PF`.
    AddPf,
    /// `OP_ADD_FP`.
    AddFp,
    /// `OP_ADD_PI`.
    AddPi,
    /// `OP_ADD_IP`.
    AddIp,
    /// `OP_ADD_PU`.
    AddPu,
    /// `OP_ADD_UP`.
    AddUp,
    /// `OP_SUB_SI`.
    SubSi,
    /// `OP_SUB_PF`.
    SubPf,
    /// `OP_SUB_PI`.
    SubPi,
    /// `OP_SUB_PU`.
    SubPu,
    /// `OP_SUB_PP`.
    SubPp,
    /// `OP_MOD_F`.
    ModF,
    /// `OP_MOD_I`.
    ModI,
    /// `OP_MOD_FI`.
    ModFi,
    /// `OP_MOD_IF`.
    ModIf,
    /// `OP_MOD_V`.
    ModV,
    /// `OP_BITXOR_F`.
    BitxorF,
    /// `OP_RSHIFT_F`.
    RshiftF,
    /// `OP_LSHIFT_F`.
    LshiftF,
    /// `OP_RSHIFT_IF`.
    RshiftIf,
    /// `OP_LSHIFT_IF`.
    LshiftIf,
    /// `OP_RSHIFT_FI`.
    RshiftFi,
    /// `OP_LSHIFT_FI`.
    LshiftFi,
    /// `OP_AND_ANY`.
    AndAny,
    /// `OP_OR_ANY`.
    OrAny,
    /// `OP_ADD_EI`.
    AddEi,
    /// `OP_ADD_EF`.
    AddEf,
    /// `OP_SUB_EI`.
    SubEi,
    /// `OP_SUB_EF`.
    SubEf,
    /// `OP_BITAND_V`.
    BitandV,
    /// `OP_BITOR_V`.
    BitorV,
    /// `OP_BITNOT_V`.
    BitnotV,
    /// `OP_BITXOR_V`.
    BitxorV,
    /// `OP_POW_F`.
    PowF,
    /// `OP_POW_I`.
    PowI,
    /// `OP_POW_FI`.
    PowFi,
    /// `OP_POW_IF`.
    PowIf,
    /// `OP_CROSS_V`.
    CrossV,
    /// `OP_EQ_FLD`.
    EqFld,
    /// `OP_NE_FLD`.
    NeFld,
    /// `OP_SPACESHIP_F`.
    SpaceshipF,
    /// `OP_SPACESHIP_S`.
    SpaceshipS,
    /// `OP_ADD_U`.
    AddU,
    /// `OP_SUB_U`.
    SubU,
    /// `OP_MUL_U`.
    MulU,
    /// `OP_MOD_U`.
    ModU,
    /// `OP_BITAND_U`.
    BitandU,
    /// `OP_BITOR_U`.
    BitorU,
    /// `OP_BITXOR_U`.
    BitxorU,
    /// `OP_BITNOT_U`.
    BitnotU,
    /// `OP_BITCLR_U`.
    BitclrU,
    /// `OP_LSHIFT_U`.
    LshiftU,
    /// `OP_GE_U`.
    GeU,
    /// `OP_GT_U`.
    GtU,
    /// `OP_EQ_U`.
    EqU,
    /// `OP_NE_U`.
    NeU,
    /// `OP_BITNOT_I64`.
    BitnotI64,
    /// `OP_BITCLR_I64`.
    BitclrI64,
    /// `OP_GE_I64`.
    GeI64,
    /// `OP_GT_I64`.
    GtI64,
    /// `OP_ADD_U64`.
    AddU64,
    /// `OP_SUB_U64`.
    SubU64,
    /// `OP_MUL_U64`.
    MulU64,
    /// `OP_MOD_U64`.
    ModU64,
    /// `OP_BITAND_U64`.
    BitandU64,
    /// `OP_BITOR_U64`.
    BitorU64,
    /// `OP_BITXOR_U64`.
    BitxorU64,
    /// `OP_BITNOT_U64`.
    BitnotU64,
    /// `OP_BITCLR_U64`.
    BitclrU64,
    /// `OP_LSHIFT_U64I`.
    LshiftU64i,
    /// `OP_GE_U64`.
    GeU64,
    /// `OP_GT_U64`.
    GtU64,
    /// `OP_EQ_U64`.
    EqU64,
    /// `OP_NE_U64`.
    NeU64,
    /// `OP_BITAND_D`.
    BitandD,
    /// `OP_BITOR_D`.
    BitorD,
    /// `OP_BITXOR_D`.
    BitxorD,
    /// `OP_BITNOT_D`.
    BitnotD,
    /// `OP_BITCLR_D`.
    BitclrD,
    /// `OP_LSHIFT_DI`.
    LshiftDi,
    /// `OP_RSHIFT_DI`.
    RshiftDi,
    /// `OP_GE_D`.
    GeD,
    /// `OP_GT_D`.
    GtD,
    /// `OP_WSTATE`.
    Wstate,
}

impl Op {
    /// Every opcode, in `pr_opcodes[]` order.
    pub const ALL: [Self; 413] = [
        Self::Done,
        Self::MulF,
        Self::MulV,
        Self::MulFv,
        Self::MulVf,
        Self::DivF,
        Self::AddF,
        Self::AddV,
        Self::SubF,
        Self::SubV,
        Self::EqF,
        Self::EqV,
        Self::EqS,
        Self::EqE,
        Self::EqFnc,
        Self::NeF,
        Self::NeV,
        Self::NeS,
        Self::NeE,
        Self::NeFnc,
        Self::LeF,
        Self::GeF,
        Self::LtF,
        Self::GtF,
        Self::LoadF,
        Self::LoadV,
        Self::LoadS,
        Self::LoadEnt,
        Self::LoadFld,
        Self::LoadFnc,
        Self::Address,
        Self::StoreF,
        Self::StoreV,
        Self::StoreS,
        Self::StoreEnt,
        Self::StoreFld,
        Self::StoreFnc,
        Self::StorepF,
        Self::StorepV,
        Self::StorepS,
        Self::StorepEnt,
        Self::StorepFld,
        Self::StorepFnc,
        Self::Return,
        Self::NotF,
        Self::NotV,
        Self::NotS,
        Self::NotEnt,
        Self::NotFnc,
        Self::IfI,
        Self::IfnotI,
        Self::Call0,
        Self::Call1,
        Self::Call2,
        Self::Call3,
        Self::Call4,
        Self::Call5,
        Self::Call6,
        Self::Call7,
        Self::Call8,
        Self::State,
        Self::Goto,
        Self::AndF,
        Self::OrF,
        Self::BitandF,
        Self::BitorF,
        Self::MulstoreF,
        Self::MulstoreVf,
        Self::MulstorepF,
        Self::MulstorepVf,
        Self::DivstoreF,
        Self::DivstorepF,
        Self::AddstoreF,
        Self::AddstoreV,
        Self::AddstorepF,
        Self::AddstorepV,
        Self::SubstoreF,
        Self::SubstoreV,
        Self::SubstorepF,
        Self::SubstorepV,
        Self::FetchGblF,
        Self::FetchGblV,
        Self::FetchGblS,
        Self::FetchGblE,
        Self::FetchGblFnc,
        Self::Cstate,
        Self::Cwstate,
        Self::Thinktime,
        Self::BitsetstoreF,
        Self::BitsetstorepF,
        Self::BitclrstoreF,
        Self::BitclrstorepF,
        Self::Rand0,
        Self::Rand1,
        Self::Rand2,
        Self::Randv0,
        Self::Randv1,
        Self::Randv2,
        Self::SwitchF,
        Self::SwitchV,
        Self::SwitchS,
        Self::SwitchE,
        Self::SwitchFnc,
        Self::Case,
        Self::Caserange,
        Self::Call1h,
        Self::Call2h,
        Self::Call3h,
        Self::Call4h,
        Self::Call5h,
        Self::Call6h,
        Self::Call7h,
        Self::Call8h,
        Self::StoreI,
        Self::StoreIf,
        Self::StoreFi,
        Self::AddI,
        Self::AddFi,
        Self::AddIf,
        Self::SubI,
        Self::SubFi,
        Self::SubIf,
        Self::ConvItof,
        Self::ConvFtoi,
        Self::LoadpItof,
        Self::LoadpFtoi,
        Self::LoadI,
        Self::StorepI,
        Self::StorepIf,
        Self::StorepFi,
        Self::BitandI,
        Self::BitorI,
        Self::MulI,
        Self::DivI,
        Self::EqI,
        Self::NeI,
        Self::IfnotS,
        Self::IfS,
        Self::NotI,
        Self::DivVf,
        Self::BitxorI,
        Self::RshiftI,
        Self::LshiftI,
        Self::Globaladdress,
        Self::AddPiw,
        Self::LoadaF,
        Self::LoadaV,
        Self::LoadaS,
        Self::LoadaEnt,
        Self::LoadaFld,
        Self::LoadaFnc,
        Self::LoadaI,
        Self::StoreP,
        Self::LoadP,
        Self::LoadpF,
        Self::LoadpV,
        Self::LoadpS,
        Self::LoadpEnt,
        Self::LoadpFld,
        Self::LoadpFnc,
        Self::LoadpI,
        Self::LeI,
        Self::GeI,
        Self::LtI,
        Self::GtI,
        Self::LeIf,
        Self::GeIf,
        Self::LtIf,
        Self::GtIf,
        Self::LeFi,
        Self::GeFi,
        Self::LtFi,
        Self::GtFi,
        Self::EqIf,
        Self::EqFi,
        Self::AddSf,
        Self::SubS,
        Self::StorepC,
        Self::LoadpC,
        Self::MulIf,
        Self::MulFi,
        Self::MulVi,
        Self::MulIv,
        Self::DivIf,
        Self::DivFi,
        Self::BitandIf,
        Self::BitorIf,
        Self::BitandFi,
        Self::BitorFi,
        Self::AndI,
        Self::OrI,
        Self::AndIf,
        Self::OrIf,
        Self::AndFi,
        Self::OrFi,
        Self::NeIf,
        Self::NeFi,
        Self::GstorepI,
        Self::GstorepF,
        Self::GstorepEnt,
        Self::GstorepFld,
        Self::GstorepS,
        Self::GstorepFnc,
        Self::GstorepV,
        Self::Gaddress,
        Self::GloadI,
        Self::GloadF,
        Self::GloadFld,
        Self::GloadEnt,
        Self::GloadS,
        Self::GloadFnc,
        Self::Boundcheck,
        Self::Unused,
        Self::Push,
        Self::Pop,
        Self::SwitchI,
        Self::GloadV,
        Self::IfF,
        Self::IfnotF,
        Self::StorefV,
        Self::StorefF,
        Self::StorefS,
        Self::StorefI,
        Self::StorepI8,
        Self::LoadpU8,
        Self::LeU,
        Self::LtU,
        Self::DivU,
        Self::RshiftU,
        Self::AddI64,
        Self::SubI64,
        Self::MulI64,
        Self::DivI64,
        Self::BitandI64,
        Self::BitorI64,
        Self::BitxorI64,
        Self::LshiftI64i,
        Self::RshiftI64i,
        Self::LeI64,
        Self::LtI64,
        Self::EqI64,
        Self::NeI64,
        Self::LeU64,
        Self::LtU64,
        Self::DivU64,
        Self::RshiftU64i,
        Self::StoreI64,
        Self::StorepI64,
        Self::StorefI64,
        Self::LoadI64,
        Self::LoadaI64,
        Self::LoadpI64,
        Self::ConvUi64,
        Self::ConvIi64,
        Self::ConvI64i,
        Self::ConvFd,
        Self::ConvDf,
        Self::ConvI64f,
        Self::ConvFi64,
        Self::ConvI64d,
        Self::ConvDi64,
        Self::AddD,
        Self::SubD,
        Self::MulD,
        Self::DivD,
        Self::LeD,
        Self::LtD,
        Self::EqD,
        Self::NeD,
        Self::StorepI16,
        Self::LoadpI16,
        Self::LoadpU16,
        Self::LoadpI8,
        Self::BitextendI,
        Self::BitextendU,
        Self::BitcopyI,
        Self::ConvUf,
        Self::ConvFu,
        Self::ConvU64d,
        Self::ConvDu64,
        Self::ConvU64f,
        Self::ConvFu64,
        Self::Numrealops,
        Self::BitsetstoreI,
        Self::BitsetstorepI,
        Self::BitclrstoreI,
        Self::MulstoreI,
        Self::DivstoreI,
        Self::AddstoreI,
        Self::SubstoreI,
        Self::MulstorepI,
        Self::DivstorepI,
        Self::AddstorepI,
        Self::SubstorepI,
        Self::MulstoreIf,
        Self::MulstorepIf,
        Self::DivstoreIf,
        Self::DivstorepIf,
        Self::AddstoreIf,
        Self::AddstorepIf,
        Self::SubstoreIf,
        Self::SubstorepIf,
        Self::MulstoreFi,
        Self::MulstorepFi,
        Self::DivstoreFi,
        Self::DivstorepFi,
        Self::AddstoreFi,
        Self::AddstorepFi,
        Self::SubstoreFi,
        Self::SubstorepFi,
        Self::MulstoreVi,
        Self::MulstorepVi,
        Self::LoadaStruct,
        Self::LoadpP,
        Self::StorepP,
        Self::BitnotF,
        Self::BitnotI,
        Self::EqP,
        Self::NeP,
        Self::LeP,
        Self::GeP,
        Self::LtP,
        Self::GtP,
        Self::AndstoreF,
        Self::BitclrF,
        Self::BitclrI,
        Self::BitclrV,
        Self::AddSi,
        Self::AddIs,
        Self::AddPf,
        Self::AddFp,
        Self::AddPi,
        Self::AddIp,
        Self::AddPu,
        Self::AddUp,
        Self::SubSi,
        Self::SubPf,
        Self::SubPi,
        Self::SubPu,
        Self::SubPp,
        Self::ModF,
        Self::ModI,
        Self::ModFi,
        Self::ModIf,
        Self::ModV,
        Self::BitxorF,
        Self::RshiftF,
        Self::LshiftF,
        Self::RshiftIf,
        Self::LshiftIf,
        Self::RshiftFi,
        Self::LshiftFi,
        Self::AndAny,
        Self::OrAny,
        Self::AddEi,
        Self::AddEf,
        Self::SubEi,
        Self::SubEf,
        Self::BitandV,
        Self::BitorV,
        Self::BitnotV,
        Self::BitxorV,
        Self::PowF,
        Self::PowI,
        Self::PowFi,
        Self::PowIf,
        Self::CrossV,
        Self::EqFld,
        Self::NeFld,
        Self::SpaceshipF,
        Self::SpaceshipS,
        Self::AddU,
        Self::SubU,
        Self::MulU,
        Self::ModU,
        Self::BitandU,
        Self::BitorU,
        Self::BitxorU,
        Self::BitnotU,
        Self::BitclrU,
        Self::LshiftU,
        Self::GeU,
        Self::GtU,
        Self::EqU,
        Self::NeU,
        Self::BitnotI64,
        Self::BitclrI64,
        Self::GeI64,
        Self::GtI64,
        Self::AddU64,
        Self::SubU64,
        Self::MulU64,
        Self::ModU64,
        Self::BitandU64,
        Self::BitorU64,
        Self::BitxorU64,
        Self::BitnotU64,
        Self::BitclrU64,
        Self::LshiftU64i,
        Self::GeU64,
        Self::GtU64,
        Self::EqU64,
        Self::NeU64,
        Self::BitandD,
        Self::BitorD,
        Self::BitxorD,
        Self::BitnotD,
        Self::BitclrD,
        Self::LshiftDi,
        Self::RshiftDi,
        Self::GeD,
        Self::GtD,
        Self::Wstate,
    ];

    /// Its `OP_*` number, its row of `pr_opcodes[]`.
    #[must_use]
    pub const fn number(self) -> usize {
        self as usize
    }

    /// Its row's `opname`, as `#pragma opcode` and FTEQCC's messages name it.
    #[must_use]
    pub fn name(self) -> &'static str {
        NAMES.get(self.number()).copied().unwrap_or_default()
    }

    /// Whether it is internal, from `OP_NUMREALOPS` on: never valid, and no `#pragma opcode`
    /// changes it (FTEF-25.2, FTEF-25.3).
    #[must_use]
    pub const fn is_internal(self) -> bool {
        self.number() >= Self::Numrealops.number()
    }

    /// The first opcode whose `opname` is `name`, compared exactly (`STRCMP`).
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.name() == name)
    }

    /// The first opcode whose `opname` is `name` in any case (`QC_strcasecmp`).
    #[must_use]
    pub fn named_in_any_case(name: &[u8]) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|op| op.name().as_bytes().eq_ignore_ascii_case(name))
    }
}

/// Each opcode's row's `opname`, in order.
const NAMES: [&str; 413] = [
    "DONE",
    "MUL_F",
    "MUL_V",
    "MUL_FV",
    "MUL_VF",
    "DIV_F",
    "ADD_F",
    "ADD_V",
    "SUB_F",
    "SUB_V",
    "EQ_F",
    "EQ_V",
    "EQ_S",
    "EQ_E",
    "EQ_FNC",
    "NE_F",
    "NE_V",
    "NE_S",
    "NE_E",
    "NE_FNC",
    "LE_F",
    "GE_F",
    "LT_F",
    "GT_F",
    "LOADF_F",
    "LOADF_V",
    "LOADF_S",
    "LOADF_ENT",
    "LOADF_FLD",
    "LOADF_FNC",
    "FLDADDRESS",
    "STORE_F",
    "STORE_V",
    "STORE_S",
    "STORE_ENT",
    "STORE_FLD",
    "STORE_FNC",
    "STOREP_F",
    "STOREP_V",
    "STOREP_S",
    "STOREP_ENT",
    "STOREP_FLD",
    "STOREP_FNC",
    "RETURN",
    "NOT_F",
    "NOT_V",
    "NOT_S",
    "NOT_ENT",
    "NOT_FNC",
    "IF",
    "IFNOT",
    "CALL0",
    "CALL1",
    "CALL2",
    "CALL3",
    "CALL4",
    "CALL5",
    "CALL6",
    "CALL7",
    "CALL8",
    "STATE",
    "GOTO",
    "AND_F",
    "OR_F",
    "BITAND",
    "BITOR",
    "MULSTORE_F",
    "MULSTORE_VF",
    "MULSTOREP_F",
    "MULSTOREP_VF",
    "DIVSTORE_F",
    "DIVSTOREP_F",
    "ADDSTORE_F",
    "ADDSTORE_V",
    "ADDSTOREP_F",
    "ADDSTOREP_V",
    "SUBSTORE_F",
    "SUBSTORE_V",
    "SUBSTOREP_F",
    "SUBSTOREP_V",
    "FETCH_GBL_F",
    "FETCH_GBL_V",
    "FETCH_GBL_S",
    "FETCH_GBL_E",
    "FETCH_GBL_FNC",
    "CSTATE",
    "CWSTATE",
    "THINKTIME",
    "BITSETSTORE_F",
    "BITSETSTOREP_F",
    "BITCLRSTORE_F",
    "BITCLRSTOREP_F",
    "RAND0",
    "RAND1",
    "RAND2",
    "RANDV0",
    "RANDV1",
    "RANDV2",
    "SWITCH_F",
    "SWITCH_V",
    "SWITCH_S",
    "SWITCH_E",
    "SWITCH_FNC",
    "CASE",
    "CASERANGE",
    "CALL1H",
    "CALL2H",
    "CALL3H",
    "CALL4H",
    "CALL5H",
    "CALL6H",
    "CALL7H",
    "CALL8H",
    "STORE_I",
    "STORE_IF",
    "STORE_FI",
    "ADD_I",
    "ADD_FI",
    "ADD_IF",
    "SUB_I",
    "SUB_FI",
    "SUB_IF",
    "CONV_IF",
    "CONV_FI",
    "CONVP_IF",
    "CONVP_FI",
    "LOADF_I",
    "STOREP_I",
    "STOREP_IF",
    "STOREP_FI",
    "BITAND_I",
    "BITOR_I",
    "MUL_I",
    "DIV_I",
    "EQ_I",
    "NE_I",
    "IFNOTS",
    "IFS",
    "NOT_I",
    "DIV_VF",
    "BITXOR_I",
    "RSHIFT_I",
    "LSHIFT_I",
    "GLOBALADDRESS",
    "ADD_PIW",
    "LOADA_F",
    "LOADA_V",
    "LOADA_S",
    "LOADA_ENT",
    "LOADA_FLD",
    "LOADA_FNC",
    "LOADA_I",
    "STORE_P",
    "LOADF_P",
    "LOADP_F",
    "LOADP_V",
    "LOADP_S",
    "LOADP_ENT",
    "LOADP_FLD",
    "LOADP_FNC",
    "LOADP_I",
    "LE_I",
    "GE_I",
    "LT_I",
    "GT_I",
    "LE_IF",
    "GE_IF",
    "LT_IF",
    "GT_IF",
    "LE_FI",
    "GE_FI",
    "LT_FI",
    "GT_FI",
    "EQ_IF",
    "EQ_FI",
    "ADD_SF",
    "SUB_S",
    "STOREP_C",
    "LOADP_C",
    "MUL_IF",
    "MUL_FI",
    "MUL_VI",
    "MUL_IV",
    "DIV_IF",
    "DIV_FI",
    "BITAND_IF",
    "BITOR_IF",
    "BITAND_FI",
    "BITOR_FI",
    "AND_I",
    "OR_I",
    "AND_IF",
    "OR_IF",
    "AND_FI",
    "OR_FI",
    "NE_IF",
    "NE_FI",
    "GSTOREP_I",
    "GSTOREP_F",
    "GSTOREP_ENT",
    "GSTOREP_FLD",
    "GSTOREP_S",
    "GSTOREP_FNC",
    "GSTOREP_V",
    "GADDRESS",
    "GLOAD_I",
    "GLOAD_F",
    "GLOAD_FLD",
    "GLOAD_ENT",
    "GLOAD_S",
    "GLOAD_FNC",
    "BOUNDCHECK",
    "UNUSED",
    "PUSH",
    "POP",
    "SWITCH_I",
    "GLOAD_V",
    "IF_F",
    "IFNOT_F",
    "STOREF_V",
    "STOREF_F",
    "STOREF_S",
    "STOREF_I",
    "STOREP_I8",
    "LOADP_U8",
    "LE_U",
    "LT_U",
    "DIV_U",
    "RSHIFT_U",
    "ADD_I64",
    "SUB_I64",
    "MUL_I64",
    "DIV_I64",
    "BITAND_I64",
    "BITOR_I64",
    "BITXOR_I64",
    "LSHIFT_I64I",
    "RSHIFT_I64I",
    "LE_I64",
    "LT_I64",
    "EQ_I64",
    "NE_I64",
    "LE_U64",
    "LT_U64",
    "DIV_U64",
    "RSHIFT_U64I",
    "STORE_I64",
    "STOREP_I64",
    "STOREF_I64",
    "LOADF_I64",
    "LOADA_I64",
    "LOADP_I64",
    "CONV_UI64",
    "CONV_II64",
    "CONV_I64I",
    "CONV_FD",
    "CONV_DF",
    "CONV_I64F",
    "CONV_FI64",
    "CONV_I64D",
    "CONV_DI64",
    "ADD_D",
    "SUB_D",
    "MUL_D",
    "DIV_D",
    "LE_D",
    "LT_D",
    "EQ_D",
    "NE_D",
    "STOREP_I16",
    "LOADP_I16",
    "LOADP_U16",
    "LOADP_I8",
    "BITEXTEND_I",
    "BITEXTEND_U",
    "BITCOPY_I",
    "CONV_UF",
    "CONV_FU",
    "CONV_U64D",
    "CONV_DU64",
    "CONV_U64F",
    "CONV_FU64",
    "OP_EMULATED",
    "BITSET_I",
    "BITSETP_I",
    "BITCLR_I",
    "MULSTORE_I",
    "DIVSTORE_I",
    "ADDSTORE_I",
    "SUBSTORE_I",
    "MULSTOREP_I",
    "DIVSTOREP_I",
    "ADDSTOREP_I",
    "SUBSTOREP_I",
    "MULSTORE_IF",
    "MULSTOREP_IF",
    "DIVSTORE_IF",
    "DIVSTOREP_IF",
    "ADDSTORE_IF",
    "ADDSTOREP_IF",
    "SUBSTORE_IF",
    "SUBSTOREP_IF",
    "MULSTORE_FI",
    "MULSTOREP_FI",
    "DIVSTORE_FI",
    "DIVSTOREP_FI",
    "ADDSTORE_FI",
    "ADDSTOREP_FI",
    "SUBSTORE_FI",
    "SUBSTOREP_FI",
    "MULSTORE_VI",
    "MULSTOREP_VI",
    "LOADA_STRUCT",
    "LOADP_P",
    "STOREP_P",
    "BITNOT_F",
    "BITNOT_I",
    "EQ_P",
    "NE_P",
    "LE_P",
    "GE_P",
    "LT_P",
    "GT_P",
    "ANDSTORE_F",
    "BITCLR_F",
    "BITCLR_I",
    "BITCLR_V",
    "ADD_SI",
    "ADD_IS",
    "ADD_PF",
    "ADD_FP",
    "ADD_PI",
    "ADD_IP",
    "ADD_PU",
    "ADD_UP",
    "SUB_SI",
    "SUB_PF",
    "SUB_PI",
    "SUB_PU",
    "SUB_PP",
    "MOD_F",
    "MOD_I",
    "MOD_FI",
    "MOD_IF",
    "MOD_V",
    "BITXOR_F",
    "RSHIFT_F",
    "LSHIFT_F",
    "RSHIFT_IF",
    "LSHIFT_IF",
    "RSHIFT_FI",
    "LSHIFT_FI",
    "AND_ANY",
    "OR_ANY",
    "ADD_EI",
    "ADD_EF",
    "SUB_EI",
    "SUB_EF",
    "BITAND_V",
    "BITOR_V",
    "BITNOT_V",
    "BITXOR_V",
    "POW_F",
    "POW_I",
    "POW_FI",
    "POW_IF",
    "CROSS_V",
    "EQ_FLD",
    "NE_FLD",
    "SPACESHIP_F",
    "SPACESHIP_S",
    "ADD_U",
    "SUB_U",
    "MUL_U",
    "MOD_U",
    "BITAND_U",
    "BITOR_U",
    "BITXOR_U",
    "BITNOT_U",
    "BITCLR_U",
    "LSHIFT_U",
    "GE_U",
    "GT_U",
    "EQ_U",
    "NE_U",
    "BITNOT_I64",
    "BITCLR_I64",
    "GE_I64",
    "GT_I64",
    "ADD_U64",
    "SUB_U64",
    "MUL_U64",
    "MOD_U64",
    "BITAND_U64",
    "BITOR_U64",
    "BITXOR_U64",
    "BITNOT_U64",
    "BITCLR_U64",
    "LSHIFT_U64I",
    "GE_U64",
    "GT_U64",
    "EQ_U64",
    "NE_U64",
    "BITAND_D",
    "BITOR_D",
    "BITXOR_D",
    "BITNOT_D",
    "BITCLR_D",
    "LSHIFT_DI",
    "RSHIFT_DI",
    "GE_D",
    "GT_D",
    "WSTATE",
];

#[cfg(test)]
mod tests {
    use super::Op;

    // spec FTEF-23.1: one opcode for each row of pr_opcodes[], in order, named by its row
    #[test]
    fn each_opcode_is_its_row() {
        for (number, op) in Op::ALL.into_iter().enumerate() {
            assert_eq!(op.number(), number);
        }
        assert_eq!(Op::named("ADD_F"), Some(Op::AddF));
        assert_eq!(Op::ConvItof.name(), "CONV_IF");
        assert_eq!(Op::named("BITCLR_I"), Some(Op::BitclrstoreI));
        assert_eq!(Op::BitclrI.name(), "BITCLR_I");
        assert_eq!(Op::named_in_any_case(b"add_f"), Some(Op::AddF));
        assert_eq!(Op::named("add_f"), None);
        assert!(!Op::ConvFu64.is_internal());
        assert!(Op::Numrealops.is_internal());
    }
}
