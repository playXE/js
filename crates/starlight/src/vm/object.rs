use super::{
    array_storage::ArrayStorage,
    attributes::*,
    class::Class,
    global::JsGlobal,
    indexed_elements::IndexedElements,
    property_descriptor::StoredSlot,
    property_descriptor::{DataDescriptor, PropertyDescriptor},
    slot::*,
    structure::Structure,
    symbol_table::{Internable, Symbol},
    value::JsValue,
    Runtime,
};
use super::{indexed_elements::MAX_VECTOR_SIZE, method_table::*};
use crate::{
    heap::{
        cell::{GcCell, GcPointer, Trace},
        SlotVisitor,
    },
    utils::align_as::AlignAs,
};
use std::{
    collections::hash_map::Entry,
    mem::{size_of, ManuallyDrop},
};
use wtf_rs::object_offsetof;
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum EnumerationMode {
    Default,
    IncludeNotEnumerable,
}
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum JsHint {
    String,
    Number,
    None,
}
pub const OBJ_FLAG_TUPLE: u32 = 0x4;
pub const OBJ_FLAG_CALLABLE: u32 = 0x2;
pub const OBJ_FLAG_EXTENSIBLE: u32 = 0x1;
pub type FixedStorage = GcPointer<ArrayStorage>;
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ObjectTag {
    Ordinary,
    Array,
    Set,
    String,
    Map,
    Number,
    Error,
    Global,
    Json,
    Function,
    Regex,
    ArrayBuffer,
    Int8Array,
    Uint8Array,
    Int16Array,
    Uint16Array,
    Int32Array,
    Uint32Array,
    Int64Array,
    Uint64Array,
    Float32Array,
    Float64Array,
    Uint8ClampedArray,
    Reflect,
    Iterator,
    ArrayIterator,
    MapIterator,
    SetIterator,
    StringIterator,
    ForInIterator,
    WeakMap,
    WeakSet,

    NormalArguments,
    StrictArguments,

    Proxy,
}

#[repr(C)]
pub struct JsObject {
    pub(crate) tag: ObjectTag,
    pub(crate) class: &'static Class,
    pub(crate) structure: GcPointer<Structure>,
    pub(crate) indexed: GcPointer<IndexedElements>,
    pub(crate) slots: FixedStorage,
    pub(crate) flags: u32,
    pub(crate) object_data_start: AlignAs<(), JsValue>,
}
impl JsObject {
    pub fn direct(&self, n: usize) -> &JsValue {
        &self.slots.at(n as _)
    }

    pub fn direct_mut(&mut self, n: usize) -> &mut JsValue {
        self.slots.at_mut(n as _)
    }
    pub fn class(&self) -> &'static Class {
        self.class
    }

    #[allow(clippy::mut_from_ref)]
    pub(crate) fn data<T>(&self) -> &mut ManuallyDrop<T> {
        unsafe {
            &mut *(self as *const Self as *mut u8)
                .add(object_offsetof!(Self, object_data_start))
                .cast::<_>()
        }
    }
    pub fn as_global(&self) -> &JsGlobal {
        assert_eq!(self.tag, ObjectTag::Global);
        &*self.data::<JsGlobal>()
    }
    pub fn as_global_mut(&mut self) -> &mut JsGlobal {
        assert_eq!(self.tag, ObjectTag::Global);
        &mut **self.data::<JsGlobal>()
    }
}
unsafe impl Trace for JsObject {
    fn trace(&self, visitor: &mut SlotVisitor) {
        self.structure.trace(visitor);
        self.slots.trace(visitor);
        self.indexed.trace(visitor);
        match self.tag {
            ObjectTag::Global => {
                self.as_global().trace(visitor);
            }

            _ => (),
        }
    }
}
impl GcCell for JsObject {
    fn compute_size(&self) -> usize {
        object_size_with_tag(self.tag)
    }
}
impl Drop for JsObject {
    fn drop(&mut self) {
        match self.tag {
            ObjectTag::Global => unsafe {
                ManuallyDrop::drop(self.data::<JsGlobal>());
            },
            _ => (),
        }
    }
}

pub fn object_size_with_tag(tag: ObjectTag) -> usize {
    let size = size_of::<JsObject>();
    match tag {
        ObjectTag::Global => size + size_of::<JsGlobal>(),
        _ => size,
    }
}

fn is_absent_descriptor(desc: &PropertyDescriptor) -> bool {
    if !desc.is_enumerable() && !desc.is_enumerable_absent() {
        return false;
    }

    if !desc.is_configurable() && !desc.is_configurable_absent() {
        return false;
    }
    if desc.is_accessor() {
        return false;
    }
    if desc.is_data() {
        return DataDescriptor { parent: *desc }.is_writable()
            && DataDescriptor { parent: *desc }.is_writable_absent();
    }
    true
}

#[allow(non_snake_case)]
impl JsObject {
    pub fn prototype(&self) -> Option<GcPointer<JsObject>> {
        self.structure.prototype()
    }

    pub fn GetNonIndexedPropertySlotMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        name: Symbol,
        slot: &mut Slot,
    ) -> bool {
        loop {
            if obj.get_own_non_indexed_property_slot(vm, name, slot) {
                break true;
            }
            match obj.prototype() {
                Some(proto) => obj = proto,
                _ => break false,
            }
        }
    }

    pub fn GetOwnNonIndexedPropertySlotMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        name: Symbol,
        slot: &mut Slot,
    ) -> bool {
        let entry = obj.structure.get(vm, name);

        if !entry.is_not_found() {
            slot.set_woffset(
                *obj.direct(entry.offset as _),
                entry.attrs as _,
                Some(obj.as_dyn()),
                entry.offset,
            );
            return true;
        }
        false
    }

    pub fn PutNonIndexedSlotMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        name: Symbol,
        val: JsValue,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<(), JsValue> {
        if !obj.can_put(vm, name, slot) {
            if throwable {
                /* let msg = JsString::new(vm, "put failed").root(vm.space());
                return Err(JsValue::new(JsTypeError::new(vm, *msg, None)));*/
                todo!()
            }

            return Ok(());
        }
        if !slot.is_not_found() {
            if let Some(base) = slot.base() {
                if GcPointer::ptr_eq(*base, obj) && slot.attributes().is_data() {
                    obj.define_own_non_indexed_property_slot(
                        vm,
                        name,
                        &*DataDescriptor::new(
                            val,
                            UNDEF_ENUMERABLE | UNDEF_CONFIGURABLE | UNDEF_WRITABLE,
                        ),
                        slot,
                        throwable,
                    )?;
                    return Ok(());
                }
            }

            if slot.attributes().is_accessor() {
                let ac = slot.accessor();
                /*let args = Arguments::new(vm, JsValue::new(obj), 1);
                let mut args = Handle::new(vm.space(), args);

                *args.at_mut(0) = val;
                return ac
                    .setter()
                    .get_object()
                    .downcast::<JsObject>()
                    .unwrap()
                    .as_function_mut()
                    .call(vm, &mut args)
                    .map(|_| ());*/
                todo!()
            }
        }
        obj.define_own_non_indexed_property_slot(
            vm,
            name,
            &*DataDescriptor::new(val, W | C | E),
            slot,
            throwable,
        )?;

        Ok(())
    }

    pub fn GetOwnIndexedPropertySlotMethod(
        obj: GcPointer<Self>,
        _vm: &mut Runtime,
        index: u32,
        slot: &mut Slot,
    ) -> bool {
        if obj.indexed.dense() && index < obj.indexed.vector.size() as u32 {
            let value = obj.indexed.vector.at(index);
            if value.is_empty() {
                return false;
            }

            slot.set_1(*value, object_data(), Some(obj.as_dyn()));
            return true;
        }
        if let Some(map) = obj.indexed.map {
            if index < obj.indexed.length() {
                let it = map.get(&index);
                if let Some(it) = it {
                    slot.set_from_slot(it, Some(obj.as_dyn()));
                    return true;
                }
            }
        }

        false
    }

    pub fn PutIndexedSlotMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        index: u32,
        val: JsValue,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<(), JsValue> {
        if index < MAX_VECTOR_SIZE as u32
            && obj.indexed.dense()
            && obj.class.method_table.GetOwnIndexedPropertySlot as usize
                == Self::GetOwnIndexedPropertySlotMethod as usize
            && (obj.prototype().is_none()
                || obj.prototype().as_ref().unwrap().has_indexed_property())
        {
            slot.mark_put_result(PutResultType::IndexedOptimized, index);
            obj.define_own_indexe_value_dense_internal(vm, index, val, false);

            return Ok(());
        }
        if !obj.can_put_indexed(vm, index, slot) {
            if throwable {
                /*  let msg = JsString::new(vm, "put failed").root(vm.space());
                return Err(JsValue::new(JsTypeError::new(vm, *msg, None)));*/
                todo!()
            }
            return Ok(());
        }
        if !slot.is_not_found() {
            if let Some(base) = slot.base() {
                if GcPointer::ptr_eq(*base, obj) && slot.attributes().is_data() {
                    obj.define_own_indexed_property_slot(
                        vm,
                        index,
                        &*DataDescriptor::new(
                            val,
                            UNDEF_ENUMERABLE | UNDEF_CONFIGURABLE | UNDEF_WRITABLE,
                        ),
                        slot,
                        throwable,
                    )?;
                    return Ok(());
                }
            }

            if slot.attributes().is_accessor() {
                /* let ac = slot.accessor();
                let args = Arguments::new(vm, JsValue::new(obj), 1);
                let mut args = Handle::new(vm.space(), args);

                *args.at_mut(0) = val;
                return ac
                    .setter()
                    .as_cell()
                    .downcast::<JsObject>()
                    .unwrap()
                    .as_function_mut()
                    .call(vm, &mut args)
                    .map(|_| ());*/
                todo!()
            }
        }

        obj.define_own_indexed_property_slot(
            vm,
            index,
            &*DataDescriptor::new(val, W | E | C),
            slot,
            throwable,
        )?;
        Ok(())
    }

    pub fn GetIndexedPropertySlotMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        index: u32,
        slot: &mut Slot,
    ) -> bool {
        loop {
            if obj.get_own_indexed_property_slot(vm, index, slot) {
                return true;
            }

            match obj.prototype() {
                Some(proto) => obj = proto,
                None => break false,
            }
        }
    }

    pub fn is_extensible(&self) -> bool {
        (self.flags & OBJ_FLAG_EXTENSIBLE) != 0
    }

    pub fn set_callable(&mut self, val: bool) {
        if val {
            self.flags |= OBJ_FLAG_CALLABLE;
        } else {
            self.flags &= !OBJ_FLAG_CALLABLE;
        }
    }

    pub fn is_callable(&self) -> bool {
        (self.flags & OBJ_FLAG_CALLABLE) != 0
    }

    // section 8.12.9 `[[DefineOwnProperty]]`
    pub fn DefineOwnNonIndexedPropertySlotMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        name: Symbol,
        desc: &PropertyDescriptor,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        if !slot.is_used() {
            obj.get_own_property_slot(vm, name, slot);
        }

        if !slot.is_not_found() {
            if let Some(base) = slot.base() {
                if GcPointer::ptr_eq(*base, obj) {
                    let mut returned = false;
                    if slot.is_defined_property_accepted(vm, desc, throwable, &mut returned)? {
                        if slot.has_offset() {
                            let old = slot.attributes();
                            slot.merge(vm, desc);
                            if old != slot.attributes() {
                                let new_struct = obj.structure.change_attributes_transition(
                                    vm,
                                    name,
                                    slot.attributes(),
                                );
                                obj.structure = new_struct;
                            }
                            *obj.direct_mut(slot.offset() as _) = slot.value();

                            slot.mark_put_result(PutResultType::Replace, slot.offset());
                        } else {
                            let mut offset = 0;
                            slot.merge(vm, desc);
                            let new_struct = obj.structure.add_property_transition(
                                vm,
                                name,
                                slot.attributes(),
                                &mut offset,
                            );
                            obj.structure = new_struct;
                            let s = obj.structure;
                            //   println!("resize to {} from {}", s.get_slots_size(), obj.slots.size());
                            obj.slots.resize(vm, s.get_slots_size() as _);

                            *obj.direct_mut(offset as _) = slot.value();
                            slot.mark_put_result(PutResultType::New, offset);
                        }
                    }
                    return Ok(returned);
                }
            }
        }

        if !obj.is_extensible() {
            if throwable {
                /*let msg = JsString::new(vm, "Object non extensible").root(vm.space());
                return Err(JsValue::new(JsTypeError::new(vm, *msg, None)));*/
                todo!()
            }

            return Ok(false);
        }

        let mut offset = 0;
        let stored = StoredSlot::new(vm, desc);
        let s = obj
            .structure
            .add_property_transition(vm, name, stored.attributes(), &mut offset);
        obj.structure = s;

        let s = obj.structure;

        obj.slots.resize(vm, s.get_slots_size() as _);

        assert!(stored.value() == desc.value());
        *obj.direct_mut(offset as _) = stored.value();
        slot.mark_put_result(PutResultType::New, offset);
        //println!("add");
        Ok(true)
    }

    pub fn DefineOwnIndexedPropertySlotMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        index: u32,
        desc: &PropertyDescriptor,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        if obj.class.method_table.GetOwnIndexedPropertySlot as usize
            != Self::GetOwnIndexedPropertySlotMethod as usize
        {
            // We should reject following case
            //   var str = new String('str');
            //   Object.defineProperty(str, '0', { value: 0 });
            if !slot.is_used() {
                obj.get_own_indexed_property_slot(vm, index, slot);
            }

            let mut returned = false;
            if !slot.is_not_found() {
                if let Some(base) = slot.base() {
                    if GcPointer::ptr_eq(*base, obj) {
                        if !slot.is_defined_property_accepted(vm, desc, throwable, &mut returned)? {
                            return Ok(returned);
                        }
                    }
                }
            }
        }

        obj.define_own_indexed_property_internal(vm, index, desc, throwable)
    }

    pub fn GetNonIndexedSlotMethod(
        obj: GcPointer<Self>,
        vm: &mut Runtime,
        name: Symbol,
        slot: &mut Slot,
    ) -> Result<JsValue, JsValue> {
        if obj.get_non_indexed_property_slot(vm, name, slot) {
            return slot.get(vm, JsValue::encode_object_value(obj.as_dyn()));
        }
        Ok(JsValue::encode_undefined_value())
    }
    pub fn GetIndexedSlotMethod(
        obj: GcPointer<Self>,
        vm: &mut Runtime,
        index: u32,
        slot: &mut Slot,
    ) -> Result<JsValue, JsValue> {
        if obj.get_indexed_property_slot(vm, index, slot) {
            return slot.get(vm, JsValue::encode_object_value(obj.as_dyn()));
        }

        Ok(JsValue::encode_undefined_value())
    }

    pub fn DeleteNonIndexedMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        name: Symbol,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        let mut slot = Slot::new();
        if !obj.get_own_property_slot(vm, name, &mut slot) {
            return Ok(true);
        }

        if !slot.attributes().is_configurable() {
            if throwable {
                /*let msg =
                    JsString::new(vm, "Can not delete non configurable property").root(vm.space());
                return Err(JsValue::new(JsTypeError::new(vm, *msg, None)));*/
                todo!()
            }
            return Ok(false);
        }

        let offset = if slot.has_offset() {
            slot.offset()
        } else {
            let entry = obj.structure.get(vm, name);
            if entry.is_not_found() {
                return Ok(true);
            }
            entry.offset
        };

        let s = obj.structure.delete_property_transition(vm, name);
        obj.structure = s;
        *obj.direct_mut(offset as _) = JsValue::encode_empty_value();
        Ok(true)
    }

    #[allow(clippy::unnecessary_unwrap)]
    pub fn delete_indexed_internal(
        &mut self,
        _vm: &mut Runtime,
        index: u32,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        if self.indexed.length() <= index {
            return Ok(true);
        }

        if self.indexed.dense() {
            if index < self.indexed.vector.size() as u32 {
                *self.indexed.vector.at_mut(index) = JsValue::encode_empty_value();
                return Ok(true);
            }

            if index < MAX_VECTOR_SIZE as u32 {
                return Ok(true);
            }
        }

        if self.indexed.map.is_none() {
            return Ok(true);
        }
        let mut map = self.indexed.map.unwrap();

        match map.entry(index) {
            Entry::Vacant(_) => Ok(true),
            Entry::Occupied(x) => {
                if !x.get().attributes().is_configurable() {
                    if throwable {
                        /*let msg = JsString::new(_vm, "trying to delete non-configurable property");
                        return Err(JsValue::new(JsTypeError::new(_vm, msg, None)));*/
                        todo!()
                    }
                    return Ok(false);
                }
                x.remove();
                if map.is_empty() {
                    self.indexed.make_dense();
                }
                Ok(true)
            }
        }
    }
    pub fn DeleteIndexedMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        index: u32,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        if obj.class.method_table.GetOwnIndexedPropertySlot as usize
            == Self::GetOwnIndexedPropertySlotMethod as usize
        {
            return obj.delete_indexed_internal(vm, index, throwable);
        }
        let mut slot = Slot::new();
        if !(obj.class.method_table.GetOwnIndexedPropertySlot)(obj, vm, index, &mut slot) {
            return Ok(true);
        }

        if !slot.attributes().is_configurable() {
            if throwable {
                /*let msg =
                    JsString::new(vm, "Can not delete non configurable property").root(vm.space());
                return Err(JsValue::new(JsTypeError::new(vm, *msg, None)));*/
                todo!()
            }
            return Ok(false);
        }

        obj.delete_indexed_internal(vm, index, throwable)
    }
    #[allow(unused_variables)]
    pub fn GetPropertyNamesMethod(
        obj: GcPointer<Self>,
        vm: &mut Runtime,
        collector: &mut dyn FnMut(Symbol, u32),
        mode: EnumerationMode,
    ) {
        obj.get_own_property_names(vm, collector, mode);
        let mut obj = obj.prototype();
        while let Some(proto) = obj {
            proto.get_own_property_names(vm, collector, mode);
            obj = proto.prototype();
        }
    }
    #[allow(unused_variables)]
    pub fn GetOwnPropertyNamesMethod(
        mut obj: GcPointer<Self>,
        vm: &mut Runtime,
        collector: &mut dyn FnMut(Symbol, u32),
        mode: EnumerationMode,
    ) {
        if obj.indexed.dense() {
            for index in 0..obj.indexed.vector.size() {
                let it = obj.indexed.vector.at(index);
                if !it.is_empty() {
                    collector(Symbol::Index(index as _), u32::MAX);
                }
            }
        }

        if let Some(map) = obj.indexed.map {
            for it in map.iter() {
                if mode == EnumerationMode::IncludeNotEnumerable
                    || it.1.attributes().is_enumerable()
                {
                    collector(Symbol::Index(*it.0), u32::MAX);
                }
            }
        }

        obj.structure.get_own_property_names(
            vm,
            mode == EnumerationMode::IncludeNotEnumerable,
            collector,
        );
    }

    /// 7.1.1 ToPrimitive
    ///
    ///
    /// 7.1.1.1 OrdinaryToPrimitive
    #[allow(unused_variables)]
    pub fn DefaultValueMethod(
        obj: GcPointer<Self>,
        vm: &mut Runtime,
        hint: JsHint,
    ) -> Result<JsValue, JsValue> {
        /* let args = Arguments::new(vm, JsValue::new(obj), 0);
        let mut args = Handle::new(vm.space(), args);

        macro_rules! try_ {
            ($sym: expr) => {
                let try_get = vm.description($sym);

                let m = obj.get(vm, $sym)?;

                if m.is_callable() {
                    let res = m
                        .as_cell()
                        .downcast::<JsObject>()
                        .unwrap()
                        .as_function_mut()
                        .call(vm, &mut args)?;
                    if res.is_primitive() || res.is_undefined_or_null() {
                        return Ok(res);
                    }
                }
            };
        }

        if hint == JsHint::String {
            try_!(Symbol::toString());
            try_!(Symbol::valueOf());
        } else {
            try_!(Symbol::valueOf());
            try_!(Symbol::toString());
        }

        let msg = JsString::new(vm, "invalid default value");
        Err(JsValue::new(JsTypeError::new(vm, msg, None)))*/
        todo!()
    }
    /*const fn get_method_table() -> MethodTable {
        js_method_table!(JsObject)
    }*/

    define_jsclass!(JsObject, Object);
    pub fn new_empty(vm: &mut Runtime) -> GcPointer<Self> {
        let structure = vm.global_data().empty_object_struct.unwrap();
        Self::new(vm, structure, Self::get_class(), ObjectTag::Ordinary)
    }
    pub fn new(
        vm: &mut Runtime,
        structure: GcPointer<Structure>,
        class: &'static Class,
        tag: ObjectTag,
    ) -> GcPointer<Self> {
        let init = IndexedElements::new(vm);
        let indexed = vm.heap().allocate(init);
        let storage = ArrayStorage::with_size(
            vm,
            structure.get_slots_size() as _,
            structure.get_slots_size() as _,
        );
        let this = Self {
            structure,
            class,

            slots: storage,
            object_data_start: AlignAs::new(()),
            indexed,
            flags: OBJ_FLAG_EXTENSIBLE,
            tag,
        };
        vm.heap().allocate(this)
    }

    pub fn tag(&self) -> ObjectTag {
        self.tag
    }
}

impl GcPointer<JsObject> {
    pub fn get_own_property_names(
        &self,
        vm: &mut Runtime,
        collector: &mut dyn FnMut(Symbol, u32),
        mode: EnumerationMode,
    ) {
        (self.class.method_table.GetOwnPropertyNames)(*self, vm, collector, mode)
    }
    pub fn get_property_names(
        &self,
        vm: &mut Runtime,
        collector: &mut dyn FnMut(Symbol, u32),
        mode: EnumerationMode,
    ) {
        (self.class.method_table.GetPropertyNames)(*self, vm, collector, mode)
    }
    pub fn put_non_indexed_slot(
        &mut self,
        vm: &mut Runtime,
        name: Symbol,
        val: JsValue,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<(), JsValue> {
        unsafe {
            (self.class.method_table.PutNonIndexedSlot)(*self, vm, name, val, slot, throwable)
        }
    }
    #[allow(clippy::wrong_self_convention)]
    /// 7.1 Type Conversion
    ///
    /// 7.1.1 ToPrimitive
    pub fn to_primitive(&mut self, vm: &mut Runtime, hint: JsHint) -> Result<JsValue, JsValue> {
        let exotic_to_prim = self.get_method(vm, "toPrimitive".intern());

        let obj = *self;
        match exotic_to_prim {
            Ok(val) => {
                // downcast_unchecked here is safe because `get_method` returns `Err` if property is not a function.
                let mut func = unsafe { val.get_object().downcast_unchecked::<JsObject>() };
                /*let f = func.as_function_mut();
                let args = Arguments::new(vm, JsValue::new(obj), 1);
                let mut args = Handle::new(vm.space(), args);
                *args.at_mut(0) = match hint {
                    JsHint::Number | JsHint::None => JsValue::new(JsString::new(vm, "number")),
                    JsHint::String => JsValue::new(JsString::new(vm, "string")),
                };

                f.call(vm, &mut args)*/
                todo!()
            }
            _ => (self.class.method_table.DefaultValue)(obj, vm, hint),
        }
    }
    pub fn delete_non_indexed(
        &mut self,
        rt: &mut Runtime,
        name: Symbol,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        (self.class.method_table.DeleteNonIndexed)(*self, rt, name, throwable)
    }
    pub fn delete(
        &mut self,
        rt: &mut Runtime,
        name: Symbol,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        match name {
            Symbol::Index(index) => self.delete_indexed(rt, index, throwable),
            name => self.delete_non_indexed(rt, name, throwable),
        }
    }
    pub fn delete_indexed(
        &mut self,
        rt: &mut Runtime,
        index: u32,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        (self.class.method_table.DeleteIndexed)(*self, rt, index, throwable)
    }

    pub fn has_indexed_property(&self) -> bool {
        let mut obj = *self;
        loop {
            if obj.structure.is_indexed() {
                return true;
            }
            match obj.prototype() {
                Some(proto) => obj = proto,
                None => break false,
            }
        }
    }
    pub fn get_non_indexed_property_slot(
        &self,
        vm: &mut Runtime,
        name: Symbol,
        slot: &mut Slot,
    ) -> bool {
        unsafe { (self.class.method_table.GetNonIndexedPropertySlot)(*self, vm, name, slot) }
    }
    pub fn get_indexed_property_slot(&self, vm: &mut Runtime, index: u32, slot: &mut Slot) -> bool {
        (self.class.method_table.GetIndexedPropertySlot)(*self, vm, index, slot)
    }

    pub fn get_own_non_indexed_property_slot(
        &self,
        vm: &mut Runtime,
        name: Symbol,
        slot: &mut Slot,
    ) -> bool {
        (self.class.method_table.GetOwnNonIndexedPropertySlot)(*self, vm, name, slot)
    }
    pub fn can_put(&self, vm: &mut Runtime, name: Symbol, slot: &mut Slot) -> bool {
        if let Symbol::Index(index) = name {
            self.can_put_indexed(vm, index, slot)
        } else {
            self.can_put_non_indexed(vm, name, slot)
        }
    }

    pub fn can_put_indexed(&self, vm: &mut Runtime, index: u32, slot: &mut Slot) -> bool {
        if self.get_indexed_property_slot(vm, index, slot) {
            if slot.attributes().is_accessor() {
                return slot.accessor().setter().is_pointer()
                    && !slot.accessor().setter().is_empty();
            } else {
                return slot.attributes().is_writable();
            }
        }
        self.is_extensible()
    }
    pub fn put_indexed_slot(
        &mut self,
        vm: &mut Runtime,
        index: u32,
        val: JsValue,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<(), JsValue> {
        unsafe { (self.class.method_table.PutIndexedSlot)(*self, vm, index, val, slot, throwable) }
    }
    pub fn put_slot(
        &mut self,
        vm: &mut Runtime,
        name: Symbol,
        val: JsValue,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<(), JsValue> {
        if let Symbol::Index(index) = name {
            self.put_indexed_slot(vm, index, val, slot, throwable)
        } else {
            self.put_non_indexed_slot(vm, name, val, slot, throwable)
        }
    }
    pub fn structure(&self) -> GcPointer<Structure> {
        self.structure
    }
    pub fn get_property_slot(&self, vm: &mut Runtime, name: Symbol, slot: &mut Slot) -> bool {
        if let Symbol::Index(index) = name {
            self.get_indexed_property_slot(vm, index, slot)
        } else {
            self.get_non_indexed_property_slot(vm, name, slot)
        }
    }

    pub fn get_property(&self, vm: &mut Runtime, name: Symbol) -> PropertyDescriptor {
        let mut slot = Slot::new();
        if self.get_property_slot(vm, name, &mut slot) {
            return slot.to_descriptor();
        }
        PropertyDescriptor::new_val(JsValue::encode_empty_value(), AttrSafe::not_found())
    }
    pub fn get_method(&self, vm: &mut Runtime, name: Symbol) -> Result<JsValue, JsValue> {
        let val = self.get(vm, name);
        match val {
            Err(e) => Err(e),
            Ok(val) => {
                /* if val.is_callable() {
                    return Ok(val);
                } else {
                    let desc = vm.description(name);
                    let msg = JsString::new(vm, format!("Property '{}' is not a method", desc));
                    Err(JsValue::new(JsTypeError::new(vm, msg, None)))
                }*/
                todo!("get method");
            }
        }
    }
    pub fn put(
        &mut self,
        vm: &mut Runtime,
        name: Symbol,
        val: JsValue,
        throwable: bool,
    ) -> Result<(), JsValue> {
        let mut slot = Slot::new();

        self.put_slot(vm, name, val, &mut slot, throwable)
    }

    pub fn can_put_non_indexed(&self, vm: &mut Runtime, name: Symbol, slot: &mut Slot) -> bool {
        if self.get_non_indexed_property_slot(vm, name, slot) {
            if slot.attributes().is_accessor() {
                if slot.attributes().is_accessor() {
                    return slot.accessor().setter().is_pointer()
                        && !slot.accessor().setter().is_empty();
                } else {
                    return slot.attributes().is_writable();
                }
            }
        }
        self.is_extensible()
    }

    pub fn has_property(&self, rt: &mut Runtime, name: Symbol) -> bool {
        let mut slot = Slot::new();
        self.get_property_slot(rt, name, &mut slot)
    }
    pub fn has_own_property(&self, rt: &mut Runtime, name: Symbol) -> bool {
        let mut slot = Slot::new();
        self.get_own_property_slot(rt, name, &mut slot)
    }
    pub fn define_own_indexed_property_slot(
        &mut self,
        vm: &mut Runtime,
        index: u32,
        desc: &PropertyDescriptor,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        (self.class.method_table.DefineOwnIndexedPropertySlot)(
            *self, vm, index, desc, slot, throwable,
        )
    }
    pub fn get_own_property_slot(&self, vm: &mut Runtime, name: Symbol, slot: &mut Slot) -> bool {
        if let Symbol::Index(index) = name {
            self.get_own_indexed_property_slot(vm, index, slot)
        } else {
            self.get_own_non_indexed_property_slot(vm, name, slot)
        }
    }
    pub fn get(&self, rt: &mut Runtime, name: Symbol) -> Result<JsValue, JsValue> {
        let mut slot = Slot::new();
        self.get_slot(rt, name, &mut slot)
    }
    pub fn get_slot(
        &self,
        rt: &mut Runtime,
        name: Symbol,
        slot: &mut Slot,
    ) -> Result<JsValue, JsValue> {
        if let Symbol::Index(index) = name {
            self.get_indexed_slot(rt, index, slot)
        } else {
            self.get_non_indexed_slot(rt, name, slot)
        }
    }

    pub fn get_indexed_slot(
        &self,
        rt: &mut Runtime,
        index: u32,
        slot: &mut Slot,
    ) -> Result<JsValue, JsValue> {
        (self.class.method_table.GetIndexedSlot)(unsafe { *self }, rt, index, slot)
    }

    pub fn get_non_indexed_slot(
        &self,
        rt: &mut Runtime,
        name: Symbol,
        slot: &mut Slot,
    ) -> Result<JsValue, JsValue> {
        (self.class.method_table.GetNonIndexedSlot)(unsafe { *self }, rt, name, slot)
    }
    pub fn get_own_indexed_property_slot(
        &self,
        vm: &mut Runtime,
        index: u32,
        slot: &mut Slot,
    ) -> bool {
        (self.class.method_table.GetOwnIndexedPropertySlot)(*self, vm, index, slot)
        //unsafe { JsObject::GetOwnIndexedPropertySlotMethod(*self, vm, index, slot) }
    }
    fn define_own_indexe_value_dense_internal(
        &mut self,
        vm: &mut Runtime,
        index: u32,
        val: JsValue,
        absent: bool,
    ) {
        if index < self.indexed.vector.size() {
            if !absent {
                *self.indexed.vector.at_mut(index) = val;
            } else {
                *self.indexed.vector.at_mut(index) = JsValue::encode_undefined_value();
            }
        } else {
            if !self.structure.is_indexed() {
                let s = self.structure.change_indexed_transition(vm);

                self.structure = s;
            }

            self.indexed.vector.resize(vm, index + 1);

            if !absent {
                *self.indexed.vector.at_mut(index) = val;
            } else {
                *self.indexed.vector.at_mut(index) = JsValue::encode_undefined_value();
            }
        }
        if index >= self.indexed.length() {
            self.indexed.set_length(index + 1);
        }
    }
    pub fn define_own_indexed_property_internal(
        &mut self,
        vm: &mut Runtime,
        index: u32,
        desc: &PropertyDescriptor,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        if index >= self.indexed.length() && !self.indexed.writable() {
            if throwable {
                todo!()
            }
            return Ok(false);
        }

        if self.indexed.dense() {
            if desc.is_default() {
                if index < MAX_VECTOR_SIZE as u32 {
                    self.define_own_indexe_value_dense_internal(
                        vm,
                        index,
                        desc.value(),
                        desc.is_value_absent(),
                    );
                    return Ok(true);
                }
            } else {
                if is_absent_descriptor(desc) {
                    if index < self.indexed.vector.size()
                        && !self.indexed.vector.at(index).is_empty()
                    {
                        if !desc.is_value_absent() {
                            *self.indexed.vector.at_mut(index) = desc.value();
                        }
                        return Ok(true);
                    }
                }

                if index < MAX_VECTOR_SIZE as u32 {
                    self.indexed.make_sparse(vm);
                }
            }
        }

        let mut sparse = self.indexed.ensure_map(vm);
        match sparse.get_mut(&index) {
            Some(entry) => {
                let mut returned = false;
                if entry.is_defined_property_accepted(vm, desc, throwable, &mut returned)? {
                    entry.merge(vm, desc);
                }
                Ok(returned)
            }
            None if !self.is_extensible() => {
                if throwable {
                    todo!()
                }
                Ok(false)
            }
            None => {
                if !self.structure.is_indexed() {
                    let s = self.structure.change_indexed_transition(vm);
                    self.structure = s;
                }
                if index >= self.indexed.length() {
                    self.indexed.set_length(index + 1);
                }
                sparse.insert(index, StoredSlot::new(vm, desc));
                Ok(true)
            }
        }
    }

    pub fn define_own_property_slot(
        &mut self,
        vm: &mut Runtime,
        name: Symbol,
        desc: &PropertyDescriptor,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        if let Symbol::Index(index) = name {
            self.define_own_indexed_property_internal(vm, index, desc, throwable)
        } else {
            self.define_own_non_indexed_property_slot(vm, name, desc, slot, throwable)
        }
    }
    pub fn define_own_non_indexed_property_slot(
        &mut self,
        vm: &mut Runtime,
        name: Symbol,
        desc: &PropertyDescriptor,
        slot: &mut Slot,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        (self.class.method_table.DefineOwnNonIndexedPropertySlot)(
            *self, vm, name, desc, slot, throwable,
        )
    }
    pub fn define_own_property(
        &mut self,
        vm: &mut Runtime,
        name: Symbol,
        desc: &PropertyDescriptor,
        throwable: bool,
    ) -> Result<bool, JsValue> {
        let mut slot = Slot::new();
        self.define_own_property_slot(vm, name, desc, &mut slot, throwable)
    }
}
