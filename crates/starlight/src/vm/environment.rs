use crate::prelude::*;
use std::{
    alloc::{alloc_zeroed, dealloc, Layout},
    any::TypeId,
    mem::size_of,
};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Variable {
    pub value: JsValue,
    pub mutable: bool,
}

#[repr(C)]
pub struct Environment {
    pub parent: Option<GcPointer<Self>>,
    pub values_ptr: *mut Variable,
    pub values_count: u32,
}

impl Environment {
    pub fn new(rt: &mut Runtime, cap: u32) -> GcPointer<Self> {
        unsafe {
            let mut ptr =
                alloc_zeroed(Layout::array::<Variable>(cap as _).unwrap()).cast::<Variable>();

            for i in 0..cap {
                ptr.add(i as _).write(Variable {
                    value: JsValue::encode_undefined_value(),
                    mutable: true,
                });
            }
            rt.heap().allocate(Self {
                parent: None,
                values_ptr: ptr,
                values_count: cap,
            })
        }
    }

    pub fn as_slice(&self) -> &[Variable] {
        unsafe { std::slice::from_raw_parts(self.values_ptr, self.values_count as _) }
    }

    pub fn as_slice_mut(&mut self) -> &mut [Variable] {
        unsafe { std::slice::from_raw_parts_mut(self.values_ptr, self.values_count as _) }
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        unsafe {
            dealloc(
                self.values_ptr.cast(),
                Layout::array::<Variable>(self.values_count as _).unwrap(),
            );
        }
    }
}

impl GcCell for Environment {
    fn deser_pair(&self) -> (usize, usize) {
        (Self::deserialize as _, Self::allocate as _)
    }
}

unsafe impl Trace for Environment {
    fn trace(&mut self, visitor: &mut dyn Tracer) {
        self.parent.trace(visitor);
        for var in self.as_slice_mut() {
            var.value.trace(visitor);
        }
    }
}

impl Deserializable for Environment {
    unsafe fn deserialize_inplace(deser: &mut Deserializer) -> Self {
        let parent = Option::<GcPointer<Self>>::deserialize_inplace(deser);
        let cap = u32::deserialize_inplace(deser);
        let mut ptr = alloc_zeroed(Layout::array::<Variable>(cap as _).unwrap()).cast::<Variable>();

        for i in 0..cap {
            let value = JsValue::deserialize_inplace(deser);
            let mutable = bool::deserialize_inplace(deser);
            ptr.add(i as _).write(Variable { value, mutable });
        }
        //let values = Vec::<(JsValue, bool)>::deserialize_inplace(deser);
        Self {
            values_ptr: ptr,
            values_count: cap,
            parent,
        }
    }
    unsafe fn deserialize(at: *mut u8, deser: &mut Deserializer) {
        at.cast::<Self>().write(Self::deserialize_inplace(deser))
    }

    unsafe fn allocate(rt: &mut Runtime, _deser: &mut Deserializer) -> *mut GcPointerBase {
        rt.heap().allocate_raw(
            vtable_of_type::<Self>() as _,
            size_of::<Self>(),
            TypeId::of::<Self>(),
        )
    }
}

impl Serializable for Environment {
    fn serialize(&self, serializer: &mut SnapshotSerializer) {
        self.parent.serialize(serializer);
        self.values_count.serialize(serializer);
        for value in self.as_slice() {
            value.value.serialize(serializer);
            value.mutable.serialize(serializer);
        }
    }
}
