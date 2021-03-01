use crate::heap::{cell::*, SlotVisitor};
use cfg_if::cfg_if;
use std::intrinsics::likely;

use super::{
    error::*,
    object::{JsHint, JsObject},
    string::JsString,
    symbol_table::*,
    Runtime,
};

pub type TagKind = u32;
pub const CMP_FALSE: i32 = 0;
pub const CMP_TRUE: i32 = 1;
pub const CMP_UNDEF: i32 = -1;
pub const FIRST_TAG: TagKind = 0xfff9;
pub const LAST_TAG: TagKind = 0xffff;
pub const EMPTY_INVALID_TAG: u32 = FIRST_TAG;
pub const UNDEFINED_NULL_TAG: u32 = FIRST_TAG + 1;
pub const BOOL_TAG: u32 = FIRST_TAG + 2;
pub const INT32_TAG: u32 = FIRST_TAG + 3;
pub const NATIVE_VALUE_TAG: u32 = FIRST_TAG + 4;
pub const STR_TAG: u32 = FIRST_TAG + 5;
pub const OBJECT_TAG: u32 = FIRST_TAG + 6;
pub const FIRST_PTR_TAG: u32 = STR_TAG;

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExtendedTag {
    Empty = EMPTY_INVALID_TAG * 2 + 1,
    Undefined = UNDEFINED_NULL_TAG * 2,
    Null = UNDEFINED_NULL_TAG * 2 + 1,
    Bool = BOOL_TAG * 2,
    Int32 = INT32_TAG * 2,
    Native1 = NATIVE_VALUE_TAG * 2,
    Native2 = NATIVE_VALUE_TAG * 2 + 1,
    Str1 = STR_TAG * 2,
    Str2 = STR_TAG * 2 + 1,
    Object1 = OBJECT_TAG * 2,
    Object2 = OBJECT_TAG * 2 + 1,
}

cfg_if!(
    if #[cfg(feature="val-as-u64")] {
/// A NaN-boxed encoded value.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct JsValue(u64);

impl JsValue {
    pub const NUM_TAG_EXP_BITS: u32 = 16;
    pub const NUM_DATA_BITS: u32 = (64 - Self::NUM_TAG_EXP_BITS);
    pub const TAG_WIDTH: u32 = 4;
    pub const TAG_MASK: u32 = (1 << Self::TAG_WIDTH) - 1;
    pub const DATA_MASK: u64 = (1 << Self::NUM_DATA_BITS as u64) - 1;
    pub const ETAG_WIDTH: u32 = 5;
    pub const ETAG_MASK: u32 = (1 << Self::ETAG_WIDTH) - 1;
    #[inline]
    pub const fn from_raw(x: u64) -> Self {
        Self(x)
    }
    #[inline]
    pub const fn get_tag(&self) -> TagKind {
        (self.0 >> Self::NUM_DATA_BITS as u64) as u32
    }
    #[inline]
    pub fn get_etag(&self) -> ExtendedTag {
        unsafe { std::mem::transmute((self.0 >> (Self::NUM_DATA_BITS as u64 - 1)) as u32) }
    }
    #[inline]
    pub const fn combine_tags(a: TagKind, b: TagKind) -> u32 {
        ((a & Self::TAG_MASK) << Self::TAG_WIDTH) | (b & Self::TAG_MASK)
    }
    #[inline]
    const fn new(val: u64, tag: TagKind) -> Self {
        Self(val | ((tag as u64) << Self::NUM_DATA_BITS))
    }
    #[inline]
    const fn new_extended(val: u64, tag: ExtendedTag) -> Self {
        Self(val | ((tag as u64) << (Self::NUM_DATA_BITS - 1)))
    }
    #[inline]
    pub const fn encode_null_ptr_object_value() -> Self {
        Self::new(0, OBJECT_TAG)
    }
    #[inline]
    pub fn encode_object_value<T: GcCell + ?Sized>(val: GcPointer<T>) -> Self {
        Self::new(unsafe {std::mem::transmute::<_,usize>(val)} as _, OBJECT_TAG)
    }
    #[inline]
    pub const fn encode_native_u32(val: u32) -> Self {
        Self::new(val as _, NATIVE_VALUE_TAG)
    }
    #[inline]
    pub fn encode_native_pointer(p: *const ()) -> Self {
        Self::new(p as _, NATIVE_VALUE_TAG)
    }
    #[inline]
    pub const fn encode_bool_value(val: bool) -> Self {
        Self::new(val as _, BOOL_TAG)
    }
    #[inline]
    pub const fn encode_null_value() -> Self {
        Self::new_extended(0, ExtendedTag::Null)
    }
    #[inline]
    pub fn encode_int32(x: i32) -> Self {
        Self::new(x as _, INT32_TAG)
    }
    #[inline]
    pub const fn encode_undefined_value() -> Self {
        Self::new_extended(0, ExtendedTag::Undefined)
    }
    #[inline]
    pub const fn encode_empty_value() -> Self {
        Self::new_extended(0, ExtendedTag::Empty)
    }
    #[inline]
    pub fn encode_f64_value(x: f64) -> Self {
        Self::from_raw(x.to_bits())
    }

    #[inline]
    pub const fn encode_nan_value() -> Self {
        Self::from_raw(0x7ff8000000000000)
    }
    #[inline]
    pub fn encode_untrusted_f64_value(val: f64) -> Self {
        if val.is_nan() {
            return Self::encode_nan_value();
        }
        Self::encode_f64_value(val)
    }

    #[inline]
    pub fn update_pointer(&self, val: *const ()) -> Self {
        Self::new(val as _, self.get_tag())
    }

    #[inline]
    pub unsafe fn unsafe_update_pointer(&mut self, val: *const ()) {
        self.0 = val as u64 | (self.get_tag() as u64) << Self::NUM_DATA_BITS as u64
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.get_etag() == ExtendedTag::Null
    }
    #[inline]
    pub fn is_undefined(&self) -> bool {
        self.get_etag() == ExtendedTag::Undefined
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.get_etag() == ExtendedTag::Empty
    }

    #[inline]
    pub fn is_native_value(&self) -> bool {
        self.get_tag() == NATIVE_VALUE_TAG
    }

    #[inline]
    pub fn is_int32(&self) -> bool {
        self.get_tag() == INT32_TAG
    }

    #[inline]
    pub fn is_bool(&self) -> bool {
        self.get_tag() == BOOL_TAG
    }

    #[inline]
    pub fn is_object(&self) -> bool {
        self.get_tag() == OBJECT_TAG
    }

    #[inline]
    pub fn is_string(&self) -> bool {
        self.get_tag() == STR_TAG
    }

    #[inline]
    pub fn is_double(&self) -> bool {
        self.0 < ((FIRST_TAG as u64) << Self::NUM_DATA_BITS as u64)
    }

    #[inline]
    pub fn is_pointer(&self) -> bool {
        self.0 >= ((FIRST_TAG as u64) << Self::NUM_DATA_BITS as u64)
    }

    #[inline]
    pub fn get_raw(&self) -> u64 {
        self.0
    }

    #[inline]
    pub fn get_pointer(&self) -> *mut () {
        assert!(self.is_pointer());
        unsafe { std::mem::transmute(self.0 & Self::DATA_MASK) }
    }
    #[inline]
    pub fn get_int32(&self) -> i32 {
        assert!(self.is_int32());
        self.0 as u32 as i32
    }
    #[inline]
    pub fn get_double(&self) -> f64 {
        f64::from_bits(self.0)
    }
    #[inline]
    pub fn get_native_value(&self) -> i64 {
        assert!(self.is_native_value());
        (((self.0 & Self::DATA_MASK as u64) as i64) << (64 - Self::NUM_DATA_BITS as i64))
            >> (64 - Self::NUM_DATA_BITS as i64)
    }

    #[inline]
    pub fn get_native_u32(&self) -> u32 {
        assert!(self.is_native_value());
        self.0 as u32
    }

    #[inline]
    pub fn get_native_ptr(&self) -> *mut () {
        assert!(self.is_native_value());
        (self.0 & Self::DATA_MASK) as *mut ()
    }

    #[inline]
    pub fn get_bool(&self) -> bool {
        assert!(self.is_bool());
        (self.0 & 0x1) != 0
    }

    #[inline]
    pub fn get_object(&self) -> GcPointer<dyn GcCell> {
        assert!(self.is_object());
        unsafe { std::mem::transmute::<_,GcPointer<dyn GcCell>>(self.0 & Self::DATA_MASK) }.clone()
    }

    #[inline]
    pub fn get_number(&self) -> f64 {
        self.get_double()
    }

    pub unsafe fn set_no_barrier(&mut self, val: Self) {
        self.0 = val.0;
    }

    pub fn is_number(&self) -> bool {
        self.is_double()
    }
}

    } else if #[cfg(feature="val-as-f64")] {
/// A NaN-boxed encoded value.

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct JsValue(f64);
impl PartialEq for JsValue {
    fn eq(&self,other: &Self) -> bool {
        self.get_raw() == other.get_raw()
    }
}
impl Eq for JsValue {}
impl JsValue {
    pub const NUM_TAG_EXP_BITS: u32 = 16;
    pub const NUM_DATA_BITS: u32 = (64 - Self::NUM_TAG_EXP_BITS);
    pub const TAG_WIDTH: u32 = 4;
    pub const TAG_MASK: u32 = (1 << Self::TAG_WIDTH) - 1;
    pub const DATA_MASK: u64 = (1 << Self::NUM_DATA_BITS as u64) - 1;
    pub const ETAG_WIDTH: u32 = 5;
    pub const ETAG_MASK: u32 = (1 << Self::ETAG_WIDTH) - 1;
    #[inline]
    pub fn from_raw(x: u64) -> Self {
        Self(f64::from_bits(x))
    }
    #[inline]
    pub fn get_tag(&self) -> TagKind {
        (self.0.to_bits() >> Self::NUM_DATA_BITS as u64) as u32
    }
    #[inline]
    pub fn get_etag(&self) -> ExtendedTag {
        unsafe { std::mem::transmute((self.0.to_bits() >> (Self::NUM_DATA_BITS as u64 - 1)) as u32) }
    }
    #[inline]
    pub fn combine_tags(a: TagKind, b: TagKind) -> u32 {
        ((a & Self::TAG_MASK) << Self::TAG_WIDTH) | (b & Self::TAG_MASK)
    }
    #[inline]
    fn new(val: u64, tag: TagKind) -> Self {
        Self(f64::from_bits(val | ((tag as u64) << Self::NUM_DATA_BITS)))
    }
    #[inline]
    fn new_extended(val: u64, tag: ExtendedTag) -> Self {
        Self(f64::from_bits(val | ((tag as u64) << (Self::NUM_DATA_BITS - 1))))
    }
    #[inline]
    pub fn encode_null_ptr_object_value() -> Self {
        Self::new(0, OBJECT_TAG)
    }
    #[inline]
    pub fn encode_object_value<T: GcCell + ?Sized>(val: GcPointer<T>) -> Self {
        Self::new(unsafe {std::mem::transmute::<_,usize>(val)} as _, OBJECT_TAG)
    }
    #[inline]
    pub fn encode_native_u32(val: u32) -> Self {
        Self::new(val as _, NATIVE_VALUE_TAG)
    }
    #[inline]
    pub fn encode_native_pointer(p: *const ()) -> Self {
        Self::new(p as _, NATIVE_VALUE_TAG)
    }
    #[inline]
    pub fn encode_bool_value(val: bool) -> Self {
        Self::new(val as _, BOOL_TAG)
    }
    #[inline]
    pub fn encode_null_value() -> Self {
        Self::new_extended(0, ExtendedTag::Null)
    }
    #[inline]
    pub fn encode_int32(x: i32) -> Self {
        Self::new(x as _, INT32_TAG)
    }
    #[inline]
    pub fn encode_undefined_value() -> Self {
        Self::new_extended(0, ExtendedTag::Undefined)
    }
    #[inline]
    pub fn encode_empty_value() -> Self {
        Self::new_extended(0, ExtendedTag::Empty)
    }
    #[inline]
    pub fn encode_f64_value(x: f64) -> Self {
        Self::from_raw(x.to_bits())
    }

    #[inline]
    pub fn encode_nan_value() -> Self {
        Self::from_raw(0x7ff8000000000000)
    }
    #[inline]
    pub fn encode_untrusted_f64_value(val: f64) -> Self {
        if val.is_nan() {
            return Self::encode_nan_value();
        }
        Self::encode_f64_value(val)
    }

    #[inline]
    pub fn update_pointer(&self, val: *const ()) -> Self {
        Self::new(val as _, self.get_tag())
    }

    #[inline]
    pub unsafe fn unsafe_update_pointer(&mut self, val: *const ()) {
        self.0 = f64::from_bits(val as u64 | (self.get_tag() as u64) << Self::NUM_DATA_BITS as u64);
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.get_etag() == ExtendedTag::Null
    }
    #[inline]
    pub fn is_undefined(&self) -> bool {
        self.get_etag() == ExtendedTag::Undefined
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.get_etag() == ExtendedTag::Empty
    }

    #[inline]
    pub fn is_native_value(&self) -> bool {
        self.get_tag() == NATIVE_VALUE_TAG
    }

    #[inline]
    pub fn is_int32(&self) -> bool {
        self.get_tag() == INT32_TAG
    }

    #[inline]
    pub fn is_bool(&self) -> bool {
        self.get_tag() == BOOL_TAG
    }

    #[inline]
    pub fn is_object(&self) -> bool {
        self.get_tag() == OBJECT_TAG
    }

    #[inline]
    pub fn is_string(&self) -> bool {
        self.get_tag() == STR_TAG
    }

    #[inline]
    pub fn is_double(&self) -> bool {
        self.0.to_bits() < ((FIRST_TAG as u64) << Self::NUM_DATA_BITS as u64)
    }

    #[inline]
    pub fn is_pointer(&self) -> bool {
        self.0.to_bits() >= ((FIRST_TAG as u64) << Self::NUM_DATA_BITS as u64)
    }

    #[inline]
    pub fn get_raw(&self) -> u64 {
        self.0.to_bits()
    }

    #[inline]
    pub fn get_pointer(&self) -> *mut () {
        assert!(self.is_pointer());
        unsafe { std::mem::transmute(self.0.to_bits() & Self::DATA_MASK) }
    }
    #[inline]
    pub fn get_int32(&self) -> i32 {
        assert!(self.is_int32());
        self.0 as u32 as i32
    }
    #[inline]
    pub fn get_double(&self) -> f64 {
        f64::from_bits(self.0.to_bits())
    }
    #[inline]
    pub fn get_native_value(&self) -> i64 {
        assert!(self.is_native_value());
        (((self.0.to_bits() & Self::DATA_MASK as u64) as i64) << (64 - Self::NUM_DATA_BITS as i64))
            >> (64 - Self::NUM_DATA_BITS as i64)
    }

    #[inline]
    pub fn get_native_u32(&self) -> u32 {
        assert!(self.is_native_value());
        self.0.to_bits() as u32
    }

    #[inline]
    pub fn get_native_ptr(&self) -> *mut () {
        assert!(self.is_native_value());
        (self.0.to_bits() & Self::DATA_MASK) as *mut ()
    }

    #[inline]
    pub fn get_bool(&self) -> bool {
        assert!(self.is_bool());
        (self.0.to_bits() & 0x1) != 0
    }

    #[inline]
    pub fn get_object(&self) -> GcPointer<dyn GcCell> {
        assert!(self.is_object());
        unsafe { std::mem::transmute::<_,GcPointer<dyn GcCell>>(self.0.to_bits() & Self::DATA_MASK) }.clone()
    }

    #[inline]
    pub fn get_number(&self) -> f64 {
        self.get_double()
    }

    pub unsafe fn set_no_barrier(&mut self, val: Self) {
        self.0 = val.0;
    }

    pub fn is_number(&self) -> bool {
        self.is_double()
    }
}

    } else {
        compile_error!("val-as-u64 or val-as-f64 should be enabled");
    }
);

unsafe impl Trace for JsValue {
    fn trace(&self, visitor: &mut SlotVisitor) {
        if self.is_pointer() && !self.is_empty() {
            self.get_object().trace(visitor);
        }
    }
}

impl JsValue {
    #[inline]
    pub unsafe fn fill(start: *mut Self, end: *mut Self, fill: JsValue) {
        let mut cur = start;
        while cur != end {
            cur.write(fill);
            cur = cur.add(1);
        }
    }

    #[inline]
    pub unsafe fn uninit_copy(
        mut first: *mut Self,
        last: *mut Self,
        mut result: *mut JsValue,
    ) -> *mut JsValue {
        while first != last {
            result.write(first.read());
            first = first.add(1);
            result = result.add(1);
        }
        result
    }

    #[inline]
    pub unsafe fn copy_backward(
        first: *mut Self,
        mut last: *mut Self,
        mut result: *mut JsValue,
    ) -> *mut JsValue {
        while first != last {
            last = last.sub(1);
            result = result.sub(1);
            result.write(last.read());
        }
        result
    }
    #[inline]
    pub unsafe fn copy(
        mut first: *mut Self,
        last: *mut Self,
        mut result: *mut JsValue,
    ) -> *mut JsValue {
        while first != last {
            result.write(first.read());
            first = first.add(1);
            result = result.add(1);
        }
        result
    }

    pub fn same_value_impl(lhs: Self, rhs: Self, _zero: bool) -> bool {
        if lhs.is_number() {
            if !rhs.is_number() {
                return false;
            }

            let lhsn = lhs.get_number();
            let rhsn = rhs.get_number();
            if lhsn == rhsn {
                return true;
            }
            return lhsn.is_nan() && rhsn.is_nan();
        }

        if !lhs.is_object() || !rhs.is_object() {
            return lhs.get_raw() == rhs.get_raw();
        }
        if lhs.is_object() && rhs.is_object() {
            if lhs.get_object().is::<JsString>() && rhs.get_object().is::<JsString>() {
                return unsafe {
                    lhs.get_object().downcast_unchecked::<JsString>().as_str()
                        == rhs.get_object().downcast_unchecked::<JsString>().as_str()
                };
            }
        }
        lhs.get_raw() == rhs.get_raw()
    }
    pub fn same_value(x: JsValue, y: JsValue) -> bool {
        Self::same_value_impl(x, y, false)
    }
    pub fn same_value_zero(lhs: Self, rhs: Self) -> bool {
        Self::same_value_impl(lhs, rhs, true)
    }
    pub fn to_object(self, rt: &mut Runtime) -> Result<GcPointer<JsObject>, JsValue> {
        if self.is_undefined() || self.is_null() {
            let msg = JsString::new(rt, "ToObject to null or undefined");
            return Err(JsValue::encode_object_value(JsTypeError::new(
                rt, msg, None,
            )));
        }
        if self.is_object() && self.get_object().is::<JsObject>() {
            return Ok(unsafe { self.get_object().downcast_unchecked() });
        }

        todo!()
    }
    pub fn is_jsobject(self) -> bool {
        self.is_object() && self.get_object().is::<JsObject>()
    }
    pub fn is_symbol(self) -> bool {
        self.is_object() && self.get_object().is::<JsSymbol>()
    }
    pub fn to_primitive(self, rt: &mut Runtime, hint: JsHint) -> Result<JsValue, JsValue> {
        if self.is_object() && self.get_object().is::<JsObject>() {
            let mut object = unsafe { self.get_object().downcast_unchecked::<JsObject>() };
            object.to_primitive(rt, hint)
        } else {
            Ok(self)
        }
    }
    fn number_compare(x: f64, y: f64) -> i32 {
        if x.is_nan() || y.is_nan() {
            return CMP_UNDEF;
        }
        if x == y {
            return CMP_FALSE;
        }
        if x < y {
            CMP_TRUE
        } else {
            CMP_FALSE
        }
    }
    pub fn get_string(&self) -> GcPointer<JsString> {
        assert!(self.is_string() || self.is_js_string());
        unsafe { self.get_object().downcast_unchecked() }
    }
    pub fn is_js_string(self) -> bool {
        self.is_object() && self.get_object().is::<JsString>()
    }

    pub fn abstract_equal(self, other: JsValue, rt: &mut Runtime) -> Result<bool, JsValue> {
        let mut lhs = self;
        let mut rhs = other;

        loop {
            if likely(lhs.is_number() && rhs.is_number()) {
                return Ok(self.get_number() == rhs.get_number());
            }

            if (lhs.is_undefined() || lhs.is_null()) && (rhs.is_undefined() && rhs.is_null()) {
                return Ok(true);
            }

            if lhs.is_string() && rhs.is_string() {
                return Ok(lhs.get_string().as_str() == rhs.get_string().as_str());
            }

            if lhs.is_symbol() && rhs.is_symbol() {
                return Ok(lhs.get_raw() == rhs.get_raw());
            }
            if lhs.is_jsobject() && rhs.is_jsobject() {
                return Ok(lhs.get_raw() == rhs.get_raw());
            }
            if lhs.is_number() && rhs.is_string() {
                rhs = JsValue::encode_f64_value(rhs.to_number(rt)?);
                continue;
            }

            if lhs.is_bool() {
                lhs = JsValue::encode_f64_value(lhs.to_number(rt)?);
                continue;
            }

            if rhs.is_bool() {
                rhs = JsValue::encode_f64_value(rhs.to_number(rt)?);
                continue;
            }

            if (lhs.is_string() || lhs.is_number()) && rhs.is_object() {
                rhs = rhs.to_primitive(rt, JsHint::None)?;
                continue;
            }

            if lhs.is_object() && (rhs.is_string() || rhs.is_number()) {
                lhs = lhs.to_primitive(rt, JsHint::None)?;
                continue;
            }
            break Ok(false);
        }
    }

    pub fn strict_equal(self, other: JsValue) -> bool {
        if likely(self.is_number() && other.is_number()) {
            return self.get_number() == other.get_number();
        }

        if !self.is_object() || !other.is_object() {
            return self.get_raw() == other.get_raw();
        }

        if self.is_string() && other.is_string() {
            return self.get_string().as_str() == other.get_string().as_str();
        }
        self.get_raw() == other.get_raw()
    }
    #[inline]
    pub fn compare(self, rhs: Self, left_first: bool, rt: &mut Runtime) -> Result<i32, JsValue> {
        let lhs = self;
        if likely(lhs.is_number() && rhs.is_number()) {
            return Ok(Self::number_compare(self.get_number(), rhs.get_number()));
        }

        let px;
        let py;
        if left_first {
            px = lhs.to_primitive(rt, JsHint::Number)?;
            py = rhs.to_primitive(rt, JsHint::Number)?;
        } else {
            py = rhs.to_primitive(rt, JsHint::Number)?;
            px = lhs.to_primitive(rt, JsHint::Number)?;
        }
        if likely(px.is_number() && py.is_number()) {
            return Ok(Self::number_compare(px.get_number(), py.get_number()));
        }
        if likely(px.is_js_string() && py.is_js_string()) {
            if px.get_string().as_str() < py.get_string().as_str() {
                Ok(CMP_TRUE)
            } else {
                Ok(CMP_FALSE)
            }
        } else {
            let nx = px.to_number(rt)?;
            let ny = py.to_number(rt)?;
            Ok(Self::number_compare(nx, ny))
        }
    }
    pub fn compare_left(self, rhs: Self, rt: &mut Runtime) -> Result<i32, JsValue> {
        Self::compare(self, rhs, true, rt)
    }
    pub fn to_number(self, rt: &mut Runtime) -> Result<f64, JsValue> {
        if self.is_double() {
            Ok(self.get_double())
        } else if self.is_object() && self.get_object().is::<JsString>() {
            let s = unsafe { self.get_object().downcast_unchecked::<JsString>() };
            if let Ok(n) = s.as_str().parse::<i32>() {
                return Ok(n as f64);
            }
            Ok(s.as_str()
                .parse::<f64>()
                .unwrap_or_else(|_| f64::from_bits(0x7ff8000000000000)))
        } else if self.is_bool() {
            Ok(self.get_bool() as u8 as f64)
        } else if self.is_null() {
            Ok(0.0)
        } else if self.is_undefined() {
            Ok(f64::from_bits(0x7ff8000000000000))
        } else if self.is_object() && self.get_object().is::<JsObject>() {
            let obj = unsafe { self.get_object().downcast_unchecked::<JsObject>() };

            match (obj.class().method_table.DefaultValue)(obj, rt, JsHint::Number) {
                Ok(val) => val.to_number(rt),
                Err(e) => Err(e),
            }
        } else {
            todo!()
        }
    }

    pub fn is_callable(&self) -> bool {
        self.is_object()
            && self
                .get_object()
                .downcast::<JsObject>()
                .map(|object| object.is_callable())
                .unwrap_or(false)
    }

    pub fn is_primitive(&self) -> bool {
        self.is_number()
            || self.is_bool()
            || (self.is_object() && self.get_object().is::<JsString>())
            || (self.is_object() && self.get_object().is::<JsSymbol>())
    }

    pub fn to_string(&self, rt: &mut Runtime) -> Result<String, JsValue> {
        if self.is_number() {
            Ok(self.get_number().to_string())
        } else if self.is_null() {
            Ok("null".to_string())
        } else if self.is_undefined() {
            Ok("undefined".to_string())
        } else if self.is_bool() {
            Ok(self.get_bool().to_string())
        } else if self.is_object() {
            let object = self.get_object();
            if let Some(jsstr) = object.clone().downcast::<JsString>() {
                return Ok(jsstr.as_str().to_owned());
            } else if let Some(mut object) = object.downcast::<JsObject>() {
                return match object.to_primitive(rt, JsHint::String) {
                    Ok(val) => val.to_string(rt),
                    Err(e) => Err(e),
                };
            }

            todo!()
        } else {
            unreachable!()
        }
    }
    pub fn to_symbol(self, rt: &mut Runtime) -> Result<Symbol, JsValue> {
        if self.is_number() {
            let n = self.get_number();
            if n as u32 as f64 == n {
                return Ok(Symbol::Index(n as u32));
            }
            return Ok(n.to_string().intern());
        }
        if self.is_string() {
            return Ok(self.get_string().as_str().intern());
        }
        if self.is_null() {
            return Ok("null".intern());
        }
        if self.is_object() && self.get_object().is::<JsSymbol>() {
            return Ok(unsafe { self.get_object().downcast_unchecked::<JsSymbol>().symbol() });
        }

        if self.get_bool() {
            if self.get_bool() {
                return Ok("true".intern());
            } else {
                return Ok("false".intern());
            }
        }

        if self.is_undefined() {
            return Ok("undefined".intern());
        }
        let mut obj = self.get_object().downcast::<JsObject>().unwrap();
        let prim = obj.to_primitive(rt, JsHint::String)?;
        prim.to_symbol(rt)
    }

    pub fn get_primitive_proto(self, vm: &mut Runtime) -> GcPointer<JsObject> {
        assert!(!self.is_empty());
        assert!(self.is_primitive());
        if self.is_string() {
            return vm.global_data().string_prototype.clone().unwrap();
        } else if self.is_number() {
            return vm.global_data().number_prototype.clone().unwrap();
        } else if self.is_bool() {
            return vm.global_data().boolean_prototype.clone().unwrap();
        } else {
            return vm.global_data().symbol_prototype.clone().unwrap();
        }
    }
}

impl From<f64> for JsValue {
    fn from(x: f64) -> Self {
        Self::encode_untrusted_f64_value(x)
    }
}

impl From<i32> for JsValue {
    fn from(x: i32) -> Self {
        Self::encode_f64_value(x as _)
    }
}

impl From<u32> for JsValue {
    fn from(x: u32) -> Self {
        Self::encode_f64_value(x as _)
    }
}
impl<T: GcCell + ?Sized> From<GcPointer<T>> for JsValue {
    fn from(x: GcPointer<T>) -> Self {
        Self::encode_object_value(x)
    }
}