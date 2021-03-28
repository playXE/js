use super::{
    attributes::*, method_table::*, object::*, property_descriptor::*, string::JsString,
    structure::*, symbol_table::*, value::JsValue, Runtime,
};
use crate::gc::cell::GcPointer;

pub struct JsError;
pub struct JsEvalError;
pub struct JsRangeError;
pub struct JsReferenceError;
pub struct JsSyntaxError;
pub struct JsTypeError;
pub struct JsURIError;
impl JsError {
    pub fn new(
        vm: &mut Runtime,
        s: GcPointer<JsString>,
        structure: Option<GcPointer<Structure>>,
    ) -> GcPointer<JsObject> {
        let stack = vm.shadowstack();
        root!(
            shape = stack,
            structure.unwrap_or_else(|| vm.global_data().error_structure.clone().unwrap())
        );
        let mut obj = JsObject::new(vm, &shape, Self::get_class(), ObjectTag::Ordinary);
        let str = JsString::new(vm, "");
        let _ = obj.define_own_property(
            vm,
            "stack".intern(),
            &*DataDescriptor::new(JsValue::new(str), W | C),
            false,
        );
        if !s.as_str().is_empty() {
            let _ = obj.define_own_property(
                vm,
                "message".intern(),
                &*DataDescriptor::new(JsValue::encode_object_value(s), W | C),
                false,
            );
        }

        obj
    }
    define_jsclass!(JsObject, Error);
}

impl JsEvalError {
    pub fn new(
        vm: &mut Runtime,
        s: GcPointer<JsString>,
        structure: Option<GcPointer<Structure>>,
    ) -> GcPointer<JsObject> {
        let stack = vm.shadowstack();
        root!(
            shape = stack,
            structure.unwrap_or_else(|| vm.global_data().eval_error_structure.clone().unwrap())
        );
        let mut obj = JsObject::new(vm, &shape, Self::get_class(), ObjectTag::Ordinary);
        let str = JsString::new(vm, "");
        let _ = obj.define_own_property(
            vm,
            "stack".intern(),
            &*DataDescriptor::new(JsValue::new(str), W | C),
            false,
        );
        if !s.as_str().is_empty() {
            let _ = obj.define_own_property(
                vm,
                "message".intern(),
                &*DataDescriptor::new(JsValue::encode_object_value(s), W | C),
                false,
            );
        }

        obj
    }
    define_jsclass_with_symbol!(JsObject, Error, EvalError);
}

impl JsRangeError {
    pub fn new(
        vm: &mut Runtime,
        s: GcPointer<JsString>,
        structure: Option<GcPointer<Structure>>,
    ) -> GcPointer<JsObject> {
        let stack = vm.shadowstack();
        root!(
            shape = stack,
            structure.unwrap_or_else(|| vm.global_data().range_error_structure.clone().unwrap())
        );
        let mut obj = JsObject::new(vm, &shape, Self::get_class(), ObjectTag::Ordinary);
        let str = JsString::new(vm, "");
        let _ = obj.define_own_property(
            vm,
            "stack".intern(),
            &*DataDescriptor::new(JsValue::new(str), W | C),
            false,
        );
        if !s.as_str().is_empty() {
            let _ = obj.define_own_property(
                vm,
                "message".intern(),
                &*DataDescriptor::new(JsValue::encode_object_value(s), W | C),
                false,
            );
        }

        obj
    }
    define_jsclass_with_symbol!(JsObject, Error, RangeError);
}

impl JsReferenceError {
    pub fn new(
        vm: &mut Runtime,
        s: GcPointer<JsString>,
        structure: Option<GcPointer<Structure>>,
    ) -> GcPointer<JsObject> {
        let stack = vm.shadowstack();
        root!(
            shape = stack,
            structure.unwrap_or_else(|| vm
                .global_data()
                .reference_error_structure
                .clone()
                .unwrap())
        );
        let mut obj = JsObject::new(vm, &shape, Self::get_class(), ObjectTag::Ordinary);
        let str = JsString::new(vm, "");
        let _ = obj.define_own_property(
            vm,
            "stack".intern(),
            &*DataDescriptor::new(JsValue::new(str), W | C),
            false,
        );
        if !s.as_str().is_empty() {
            let _ = obj.define_own_property(
                vm,
                "message".intern(),
                &*DataDescriptor::new(JsValue::encode_object_value(s), W | C),
                false,
            );
        }

        obj
    }
    define_jsclass_with_symbol!(JsObject, Error, ReferenceError);
}

impl JsSyntaxError {
    pub fn new(
        vm: &mut Runtime,
        s: GcPointer<JsString>,
        structure: Option<GcPointer<Structure>>,
    ) -> GcPointer<JsObject> {
        let stack = vm.shadowstack();
        root!(
            shape = stack,
            structure.unwrap_or_else(|| vm.global_data().syntax_error_structure.clone().unwrap())
        );
        let mut obj = JsObject::new(vm, &shape, Self::get_class(), ObjectTag::Ordinary);
        let str = JsString::new(vm, "");
        let _ = obj.define_own_property(
            vm,
            "stack".intern(),
            &*DataDescriptor::new(JsValue::new(str), W | C),
            false,
        );
        if !s.as_str().is_empty() {
            let _ = obj.define_own_property(
                vm,
                "message".intern(),
                &*DataDescriptor::new(JsValue::encode_object_value(s), W | C),
                false,
            );
        }

        obj
    }
    define_jsclass_with_symbol!(JsObject, Error, SyntaxError);
}

impl JsTypeError {
    pub fn new(
        vm: &mut Runtime,
        s: GcPointer<JsString>,
        structure: Option<GcPointer<Structure>>,
    ) -> GcPointer<JsObject> {
        let stack = vm.shadowstack();
        root!(
            shape = stack,
            structure.unwrap_or_else(|| vm.global_data().type_error_structure.clone().unwrap())
        );
        let mut obj = JsObject::new(vm, &shape, Self::get_class(), ObjectTag::Ordinary);
        let str = JsString::new(vm, "");
        let _ = obj.define_own_property(
            vm,
            "stack".intern(),
            &*DataDescriptor::new(JsValue::new(str), W | C),
            false,
        );
        if !s.as_str().is_empty() {
            let _ = obj.define_own_property(
                vm,
                "message".intern(),
                &*DataDescriptor::new(JsValue::encode_object_value(s), W | C),
                false,
            );
        }

        obj
    }
    define_jsclass_with_symbol!(JsObject, Error, TypeError);
}
