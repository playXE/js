use self::{frame::CallFrame, stack::Stack};
use super::{
    arguments::*, array::*, code_block::CodeBlock, error::JsTypeError, error::*,
    function::JsVMFunction, object::*, slot::*, string::JsString, structure::*, symbol_table::*,
    value::*, Runtime,
};
use crate::root;
use crate::{
    bytecode::opcodes::Opcode,
    gc::{
        cell::{GcCell, GcPointer, Trace},
        snapshot::deserializer::Deserializable,
    },
};
use crate::{bytecode::*, gc::cell::Tracer};
use std::{
    hint::unreachable_unchecked,
    intrinsics::{likely, unlikely},
};
use wtf_rs::unwrap_unchecked;
pub mod frame;
pub mod stack;

impl Runtime {
    pub(crate) fn perform_vm_call(
        &mut self,
        func: &JsVMFunction,
        env: JsValue,
        args_: &Arguments,
    ) -> Result<JsValue, JsValue> {
        let stack = self.shadowstack();
        root!(scope = stack, unsafe {
            env.get_object().downcast_unchecked::<JsObject>()
        });
        root!(
            structure = stack,
            Structure::new_indexed(self, Some(*scope), false)
        );

        root!(
            nscope = stack,
            JsObject::new(self, &structure, JsObject::get_class(), ObjectTag::Ordinary)
        );

        let mut i = 0;
        for p in func.code.params.iter() {
            let _ = nscope
                .put(self, *p, args_.at(i), false)
                .unwrap_or_else(|_| unsafe { unreachable_unchecked() });

            i += 1;
        }

        if let Some(rest) = func.code.rest_param {
            let mut args_arr = JsArray::new(self, args_.size() as u32 - i as u32);
            let mut ai = 0;
            for ix in i..args_.size() {
                args_arr.put_indexed_slot(
                    self,
                    ai as _,
                    args_.at(ix as _),
                    &mut Slot::new(),
                    false,
                )?;
                ai += 1;
            }
            nscope.put(self, rest, JsValue::encode_object_value(args_arr), false)?;
        }
        root!(
            vscope = stack,
            if func.code.top_level {
                self.global_object()
            } else {
                *nscope
            }
        );
        for val in func.code.variables.iter() {
            vscope.put(self, *val, JsValue::encode_undefined_value(), false)?;
        }
        if func.code.use_arguments {
            let mut args = JsArguments::new(
                self,
                nscope.clone(),
                &func.code.params,
                args_.size() as _,
                args_.values,
            );

            for k in i..args_.size() {
                args.put(self, Symbol::Index(k as _), args_.at(k), false)?;
            }

            let _ = nscope.put(
                self,
                "arguments".intern(),
                JsValue::encode_object_value(args),
                false,
            )?;
        }
        let _this = if func.code.strict && !args_.this.is_object() {
            JsValue::encode_undefined_value()
        } else {
            if args_.this.is_undefined() {
                JsValue::encode_object_value(self.global_object())
            } else {
                args_.this
            }
        };

        unsafe {
            eval_internal(
                self,
                func.code,
                &func.code.code[0] as *const u8 as *mut u8,
                _this,
                args_.ctor_call,
                *nscope,
            )
        }
    }
}
#[inline(never)]
unsafe fn eval_internal(
    rt: &mut Runtime,
    code: GcPointer<CodeBlock>,
    ip: *mut u8,
    this: JsValue,
    ctor: bool,
    scope: GcPointer<JsObject>,
) -> Result<JsValue, JsValue> {
    let frame = rt.stack.new_frame();
    if frame.is_none() {
        let msg = JsString::new(rt, "stack overflow");
        return Err(JsValue::encode_object_value(JsRangeError::new(
            rt, msg, None,
        )));
    }
    let frame = unwrap_unchecked(frame);
    (*frame).code_block = Some(code);
    (*frame).this = this;
    (*frame).env = JsValue::encode_object_value(scope);
    (*frame).ctor = ctor;
    (*frame).exit_on_return = true;
    (*frame).ip = ip;

    loop {
        let result = eval(rt, frame);
        match result {
            Ok(value) => return Ok(value),
            Err(e) => {
                if let Some((env, ip)) = (*frame).try_stack.pop() {
                    (*frame).env = env;
                    (*frame).ip = ip;
                    (*frame).push(e);
                    continue;
                }

                return Err(e);
            }
        }
    }
}

pub unsafe fn eval(rt: &mut Runtime, frame: *mut CallFrame) -> Result<JsValue, JsValue> {
    rt.gc().collect_if_necessary();
    let mut ip = (*frame).ip;

    let mut frame: &'static mut CallFrame = &mut *frame;
    let stack = &mut rt.stack as *mut Stack;
    let stack = &mut *stack;
    let gcstack = rt.shadowstack();
    loop {
        let opcode = ip.cast::<Opcode>().read_unaligned();
        ip = ip.add(1);
        #[cfg(feature = "perf")]
        {
            rt.perf.get_perf(opcode as u8);
        }
        stack.cursor = frame.sp;
        match opcode {
            Opcode::OP_NOP => {}
            Opcode::OP_GET_VAR => {
                let name = ip.cast::<u32>().read_unaligned();
                ip = ip.add(4);
                let fdbk = ip.cast::<u32>().read_unaligned();
                ip = ip.add(4);
                let name = *unwrap_unchecked((*frame).code_block)
                    .names
                    .get_unchecked(name as usize);
                let value = get_var(rt, name, frame, fdbk)?;

                frame.push(value);
            }
            Opcode::OP_SET_VAR => {
                let val = frame.pop();
                let name = ip.cast::<u32>().read_unaligned();
                ip = ip.add(4);
                let fdbk = ip.cast::<u32>().read_unaligned();
                ip = ip.add(4);
                let name = *unwrap_unchecked((*frame).code_block)
                    .names
                    .get_unchecked(name as usize);
                set_var(rt, frame, name, fdbk, val)?;
            }
            Opcode::OP_PUSH_ENV => {
                let _fdbk = ip.cast::<u32>().read_unaligned();
                ip = ip.add(4);

                let structure = Structure::new_indexed(rt, Some(frame.env.get_jsobject()), false);

                let env = JsObject::new(rt, &structure, JsObject::get_class(), ObjectTag::Ordinary);
                frame.env = JsValue::encode_object_value(env);
            }
            Opcode::OP_POP_ENV => {
                let mut env = frame.env.get_jsobject();

                frame.env = JsValue::encode_object_value(
                    env.prototype().copied().expect("no environments left"),
                );
            }
            Opcode::OP_JMP => {
                rt.gc().collect_if_necessary();
                let offset = ip.cast::<i32>().read();
                ip = ip.add(4);
                ip = ip.offset(offset as isize);
            }
            Opcode::OP_JMP_IF_FALSE => {
                let offset = ip.cast::<i32>().read();
                ip = ip.add(4);
                let value = frame.pop();
                if !value.to_boolean() {
                    ip = ip.offset(offset as _);
                }
            }
            Opcode::OP_JMP_IF_TRUE => {
                let offset = ip.cast::<i32>().read();
                ip = ip.add(4);
                let value = frame.pop();
                if value.to_boolean() {
                    ip = ip.offset(offset as _);
                }
            }

            Opcode::OP_POP => {
                frame.pop();
            }
            Opcode::OP_PUSH_TRUE => {
                frame.push(JsValue::encode_bool_value(true));
            }
            Opcode::OP_PUSH_FALSE => {
                frame.push(JsValue::encode_bool_value(false));
            }
            Opcode::OP_PUSH_LITERAL => {
                let ix = ip.cast::<u32>().read();
                ip = ip.add(4);
                let constant = unwrap_unchecked(frame.code_block).literals[ix as usize];
                //assert!(constant.is_jsstring());
                frame.push(constant);
            }
            Opcode::OP_PUSH_THIS => {
                frame.push(frame.this);
            }
            Opcode::OP_PUSH_INT => {
                let int = ip.cast::<i32>().read();

                ip = ip.add(4);
                frame.push(JsValue::encode_f64_value(int as f64));
            }
            Opcode::OP_PUSH_NAN => {
                frame.push(JsValue::encode_nan_value());
            }
            Opcode::OP_PUSH_NULL => {
                frame.push(JsValue::encode_null_value());
            }
            Opcode::OP_RET => {
                rt.gc().collect_if_necessary();
                let mut value = if frame.sp <= frame.limit {
                    JsValue::encode_undefined_value()
                } else {
                    frame.pop()
                };

                if frame.ctor && !value.is_jsobject() {
                    value = frame.this;
                }
                rt.stack.pop_frame().unwrap();
                //if frame.exit_on_return || frame.prev.is_null() {
                return Ok(value);
                /*}
                let _ = rt.stack.pop_frame().unwrap();
                frame = &mut *rt.stack.current;
                ip = frame.ip;
                frame.push(value);*/
            }
            Opcode::OP_ADD => {
                // let profile = &mut *ip.cast::<ArithProfile>();
                // ip = ip.add(size_of::<ArithProfile>());

                let lhs = frame.pop();
                let rhs = frame.pop();
                // profile.observe_lhs_and_rhs(lhs, rhs);

                if likely(lhs.is_number() && rhs.is_number()) {
                    let result = JsValue::encode_f64_value(lhs.get_number() + rhs.get_number());

                    frame.push(result);
                    continue;
                }

                let lhs = lhs.to_primitive(rt, JsHint::None)?;
                let rhs = rhs.to_primitive(rt, JsHint::None)?;

                if lhs.is_jsstring() || rhs.is_jsstring() {
                    #[inline(never)]
                    fn concat(
                        rt: &mut Runtime,
                        lhs: JsValue,
                        rhs: JsValue,
                    ) -> Result<JsValue, JsValue> {
                        let lhs = lhs.to_string(rt)?;
                        let rhs = rhs.to_string(rt)?;
                        let string = format!("{}{}", lhs, rhs);
                        Ok(JsValue::encode_object_value(JsString::new(rt, string)))
                    }

                    let result = concat(rt, lhs, rhs)?;
                    frame.push(result);
                } else {
                    let lhs = lhs.to_number(rt)?;
                    let rhs = rhs.to_number(rt)?;
                    frame.push(JsValue::encode_f64_value(lhs + rhs));
                }
            }
            Opcode::OP_SUB => {
                //let profile = &mut *ip.cast::<ArithProfile>();
                //ip = ip.add(size_of::<ArithProfile>());

                let lhs = frame.pop();
                let rhs = frame.pop();
                if likely(lhs.is_number() && rhs.is_number()) {
                    //profile.lhs_saw_number();
                    //profile.rhs_saw_number();
                    frame.push(JsValue::encode_f64_value(
                        lhs.get_number() - rhs.get_number(),
                    ));

                    continue;
                }
                // profile.observe_lhs_and_rhs(lhs, rhs);
                let lhs = lhs.to_number(rt)?;
                let rhs = rhs.to_number(rt)?;
                frame.push(JsValue::encode_f64_value(lhs - rhs));
            }
            Opcode::OP_DIV => {
                //let profile = &mut *ip.cast::<ArithProfile>();
                //ip = ip.add(size_of::<ArithProfile>());

                let lhs = frame.pop();
                let rhs = frame.pop();
                if likely(lhs.is_number() && rhs.is_number()) {
                    //    profile.lhs_saw_number();
                    //    profile.rhs_saw_number();
                    frame.push(JsValue::encode_f64_value(
                        lhs.get_number() / rhs.get_number(),
                    ));
                    continue;
                }
                //profile.observe_lhs_and_rhs(lhs, rhs);
                let lhs = lhs.to_number(rt)?;
                let rhs = rhs.to_number(rt)?;
                frame.push(JsValue::encode_f64_value(lhs / rhs));
            }
            Opcode::OP_MUL => {
                //let profile = &mut *ip.cast::<ArithProfile>();
                //ip = ip.add(size_of::<ArithProfile>());

                let lhs = frame.pop();
                let rhs = frame.pop();
                if likely(lhs.is_number() && rhs.is_number()) {
                    //  profile.lhs_saw_number();
                    //  profile.rhs_saw_number();

                    frame.push(JsValue::encode_f64_value(
                        lhs.get_number() * rhs.get_number(),
                    ));
                    continue;
                }
                //profile.observe_lhs_and_rhs(lhs, rhs);
                let lhs = lhs.to_number(rt)?;
                let rhs = rhs.to_number(rt)?;
                frame.push(JsValue::encode_f64_value(lhs * rhs));
            }
            Opcode::OP_REM => {
                //let profile = &mut *ip.cast::<ArithProfile>();
                //ip = ip.add(size_of::<ArithProfile>());

                let lhs = frame.pop();
                let rhs = frame.pop();

                if likely(lhs.is_number() && rhs.is_number()) {
                    //  profile.lhs_saw_number();
                    //  profile.rhs_saw_number();
                    frame.push(JsValue::encode_f64_value(
                        lhs.get_number() % rhs.get_number(),
                    ));
                    continue;
                }
                // profile.observe_lhs_and_rhs(lhs, rhs);
                let lhs = lhs.to_number(rt)?;
                let rhs = rhs.to_number(rt)?;
                frame.push(JsValue::encode_f64_value(lhs % rhs));
            }
            Opcode::OP_SHL => {
                let lhs = frame.pop();
                let rhs = frame.pop();

                let left = lhs.to_int32(rt)?;
                let right = rhs.to_uint32(rt)?;
                frame.push(JsValue::encode_f64_value((left << (right & 0x1f)) as f64));
            }
            Opcode::OP_SHR => {
                let lhs = frame.pop();
                let rhs = frame.pop();

                let left = lhs.to_int32(rt)?;
                let right = rhs.to_uint32(rt)?;
                frame.push(JsValue::encode_f64_value((left >> (right & 0x1f)) as f64));
            }

            Opcode::OP_USHR => {
                let lhs = frame.pop();
                let rhs = frame.pop();

                let left = lhs.to_uint32(rt)?;
                let right = rhs.to_uint32(rt)?;
                frame.push(JsValue::encode_f64_value((left >> (right & 0x1f)) as f64));
            }
            Opcode::OP_LESS => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                //    println!("{} {}", lhs.get_number(), rhs.get_number());
                frame.push(JsValue::encode_bool_value(
                    lhs.compare(rhs, true, rt)? == CMP_TRUE,
                ));
            }
            Opcode::OP_LESSEQ => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                frame.push(JsValue::encode_bool_value(
                    rhs.compare(lhs, false, rt)? == CMP_FALSE,
                ));
            }

            Opcode::OP_GREATER => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                frame.push(JsValue::encode_bool_value(
                    rhs.compare(lhs, false, rt)? == CMP_TRUE,
                ));
            }
            Opcode::OP_GREATEREQ => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                frame.push(JsValue::encode_bool_value(
                    lhs.compare(rhs, true, rt)? == CMP_FALSE,
                ));
            }

            Opcode::OP_CALL => {
                rt.gc().collect_if_necessary();
                let argc = ip.cast::<u32>().read();
                ip = ip.add(4);
                let mut func = frame.pop();
                let mut this = frame.pop();

                let args_start = frame.sp.sub(argc as _);
                let mut args = std::slice::from_raw_parts_mut(args_start, argc as _);
                if !func.is_callable() {
                    let msg = JsString::new(rt, "not a callable object");
                    return Err(JsValue::encode_object_value(JsTypeError::new(
                        rt, msg, None,
                    )));
                }
                root!(func_object = gcstack, func.get_jsobject());
                let func = func_object.as_function_mut();

                root!(
                    args_ = gcstack,
                    Arguments::from_array_storage(rt, this, &mut args)
                );

                let result = func.call(rt, &mut args_)?;
                frame.sp = args_start;
                frame.push(result);
            }
            Opcode::OP_NEW => {
                rt.gc().collect_if_necessary();
                let argc = ip.cast::<u32>().read();
                ip = ip.add(4);

                let mut func = frame.pop();
                let mut this = frame.pop();

                let args_start = frame.sp.sub(argc as _);
                let mut args = std::slice::from_raw_parts_mut(args_start, argc as _);

                if unlikely(!func.is_callable()) {
                    let msg = JsString::new(rt, "not a callable object");
                    return Err(JsValue::encode_object_value(JsTypeError::new(
                        rt, msg, None,
                    )));
                }

                root!(func_object = gcstack, func.get_jsobject());
                let map = func_object.func_construct_map(rt)?;
                let func = func_object.as_function_mut();
                root!(
                    args_ = gcstack,
                    Arguments::from_array_storage(rt, this, &mut args)
                );
                args_.ctor_call = true;
                let result = func.construct(rt, &mut args_, Some(map))?;
                frame.sp = args_start;
                frame.push(result);
            }

            Opcode::OP_DUP => {
                let v1 = frame.pop();
                frame.push(v1);
                frame.push(v1);
            }
            Opcode::OP_SWAP => {
                let v1 = frame.pop();
                let v2 = frame.pop();
                frame.push(v1);
                frame.push(v2);
            }
            Opcode::OP_NEG => {
                let v1 = frame.pop();
                if v1.is_number() {
                    frame.push(JsValue::encode_f64_value(-v1.get_number()));
                } else {
                    let n = v1.to_number(rt)?;
                    frame.push(JsValue::encode_f64_value(-n));
                }
            }
            Opcode::OP_GET_BY_ID => {
                let name = ip.cast::<u32>().read_unaligned();
                let name = *unwrap_unchecked(frame.code_block)
                    .names
                    .get_unchecked(name as usize);
                ip = ip.add(4);
                let fdbk = ip.cast::<u32>().read_unaligned();
                ip = ip.add(4);
                let object = frame.pop();
                if likely(object.is_jsobject()) {
                    root!(obj = gcstack, object.get_jsobject());
                    if likely(rt.options.inline_caches) {
                        if let TypeFeedBack::PropertyCache { structure, offset } =
                            unwrap_unchecked(frame.code_block)
                                .feedback
                                .get_unchecked(fdbk as usize)
                        {
                            if let Some(structure) = structure.upgrade() {
                                if GcPointer::ptr_eq(&structure, &obj.structure()) {
                                    frame.push(*obj.direct(*offset as _));
                                    continue;
                                }
                            }
                        }
                    }

                    let mut slot = Slot::new();
                    let found = obj.get_property_slot(rt, name, &mut slot);
                    if rt.options.inline_caches && slot.is_load_cacheable() {
                        *unwrap_unchecked(frame.code_block)
                            .feedback
                            .get_unchecked_mut(fdbk as usize) = TypeFeedBack::PropertyCache {
                            structure: rt.gc().make_weak(
                                slot.base()
                                    .unwrap()
                                    .downcast_unchecked::<JsObject>()
                                    .structure(),
                            ),

                            offset: slot.offset(),
                        }
                    }
                    if found {
                        frame.push(slot.get(rt, object)?);
                    } else {
                        frame.push(JsValue::encode_undefined_value());
                    }
                    continue;
                }

                frame.push(get_by_id_slow(rt, name, object)?)
            }
            Opcode::OP_PUT_BY_ID => {
                let name = ip.cast::<u32>().read_unaligned();
                let name = *unwrap_unchecked(frame.code_block)
                    .names
                    .get_unchecked(name as usize);
                ip = ip.add(4);
                let fdbk = ip.cast::<u32>().read_unaligned();
                ip = ip.add(4);

                let object = frame.pop();
                let value = frame.pop();
                if likely(object.is_jsobject()) {
                    let mut obj = object.get_jsobject();
                    if true {
                        if let TypeFeedBack::PropertyCache { structure, offset } =
                            unwrap_unchecked(frame.code_block)
                                .feedback
                                .get_unchecked(fdbk as usize)
                        {
                            if let Some(structure) = structure.upgrade() {
                                if GcPointer::ptr_eq(&structure, &obj.structure()) {
                                    *obj.direct_mut(*offset as usize) = value;

                                    continue;
                                }
                            }
                        }
                    }

                    let mut slot = Slot::new();

                    obj.put_slot(
                        rt,
                        name,
                        value,
                        &mut slot,
                        unwrap_unchecked(frame.code_block).strict,
                    )?;

                    if slot.is_put_cacheable() {
                        *unwrap_unchecked(frame.code_block)
                            .feedback
                            .get_unchecked_mut(fdbk as usize) = TypeFeedBack::PropertyCache {
                            structure: rt.gc().make_weak(obj.structure()),
                            offset: slot.offset(),
                        };
                    }
                } else {
                    eprintln!("Internal waning: PUT_BY_ID on primitives is not implemented yet");
                }
            }
            Opcode::OP_EQ => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                frame.push(JsValue::encode_bool_value(lhs.abstract_equal(rhs, rt)?));
            }
            Opcode::OP_STRICTEQ => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                frame.push(JsValue::encode_bool_value(lhs.strict_equal(rhs)));
            }
            Opcode::OP_NEQ => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                frame.push(JsValue::encode_bool_value(!lhs.abstract_equal(rhs, rt)?));
            }
            Opcode::OP_NSTRICTEQ => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                frame.push(JsValue::encode_bool_value(!lhs.strict_equal(rhs)));
            }
            Opcode::OP_INSTANCEOF => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                if unlikely(!rhs.is_jsobject()) {
                    let msg = JsString::new(rt, "'instanceof' requires object");
                    return Err(JsValue::encode_object_value(JsTypeError::new(
                        rt, msg, None,
                    )));
                }

                root!(robj = gcstack, rhs.get_jsobject());
                root!(robj2 = gcstack, *robj);
                if unlikely(!robj.is_callable()) {
                    let msg = JsString::new(rt, "'instanceof' requires constructor");
                    return Err(JsValue::encode_object_value(JsTypeError::new(
                        rt, msg, None,
                    )));
                }

                frame.push(JsValue::encode_bool_value(
                    robj.as_function().has_instance(&mut robj2, rt, lhs)?,
                ));
            }
            Opcode::OP_IN => {
                let lhs = frame.pop();
                let rhs = frame.pop();
                if unlikely(!rhs.is_jsobject()) {
                    let msg = JsString::new(rt, "'in' requires object");
                    return Err(JsValue::encode_object_value(JsTypeError::new(
                        rt, msg, None,
                    )));
                }
                let sym = lhs.to_symbol(rt)?;
                frame.push(JsValue::encode_bool_value(
                    rhs.get_jsobject().has_own_property(rt, sym),
                ));
            }
            Opcode::OP_THROW => {
                let val = frame.pop();
                return Err(val);
            }
            Opcode::OP_GET_ENV => {
                let name = ip.cast::<u32>().read_unaligned();
                ip = ip.add(4);
                let name = *unwrap_unchecked((*frame).code_block)
                    .names
                    .get_unchecked(name as usize);
                let result = 'search: loop {
                    let mut current = Some((*frame).env.get_jsobject());
                    while let Some(env) = current {
                        if (Env { record: env }).has_own_variable(rt, name) {
                            break 'search Some(env);
                        }
                        current = env.prototype().copied();
                    }
                    break 'search None;
                };

                match result {
                    Some(env) => frame.push(JsValue::encode_object_value(env)),
                    None => frame.push(JsValue::encode_undefined_value()),
                }
            }

            Opcode::OP_SET_GLOBAL => {
                let val = frame.pop();
                let name = ip.cast::<u32>().read_unaligned();

                ip = ip.add(4);
                let name = *unwrap_unchecked((*frame).code_block)
                    .names
                    .get_unchecked(name as usize);

                rt.global_object()
                    .put(rt, name, val, unwrap_unchecked(frame.code_block).strict)?;
            }
            Opcode::OP_GET_GLOBAL => {
                let name = ip.cast::<u32>().read_unaligned();

                ip = ip.add(4);
                let name = *unwrap_unchecked((*frame).code_block)
                    .names
                    .get_unchecked(name as usize);

                let val = rt.global_object().get(rt, name)?;
                frame.push(val);
            }
            Opcode::OP_GLOBALTHIS => {
                let global = rt.global_object();
                frame.push(JsValue::encode_object_value(global));
            }

            Opcode::OP_NEWOBJECT => {
                let obj = JsObject::new_empty(rt);
                frame.push(JsValue::encode_object_value(obj));
            }
            Opcode::OP_PUT_BY_VAL => {
                let object = frame.pop();
                let key = frame.pop().to_symbol(rt)?;
                let value = frame.pop();
                if likely(object.is_jsobject()) {
                    let mut obj = object.get_jsobject();
                    obj.put(rt, key, value, unwrap_unchecked(frame.code_block).strict)?;
                } else {
                    eprintln!("Internal waning: PUT_BY_VAL on primitives is not implemented yet");
                }
            }
            Opcode::OP_GET_BY_VAL => {
                let object = frame.pop();
                let key = frame.pop().to_symbol(rt)?;
                let mut slot = Slot::new();
                let value = object.get_slot(rt, key, &mut slot)?;

                frame.push(value);
            }

            Opcode::OP_PUSH_CATCH => {
                let offset = ip.cast::<i32>().read();
                ip = ip.add(4);
                let env = frame.env;

                frame.try_stack.push((env, ip.offset(offset as isize)));
            }
            Opcode::OP_POP_CATCH => {
                frame.try_stack.pop();
            }

            Opcode::OP_LOGICAL_NOT => {
                let val = frame.pop();
                frame.push(JsValue::encode_bool_value(!val.to_boolean()));
            }
            Opcode::OP_NOT => {
                let v1 = frame.pop();
                if v1.is_number() {
                    let n = v1.get_number() as i32;
                    frame.push(JsValue::encode_f64_value((!n) as _));
                } else {
                    let n = v1.to_number(rt)? as i32;
                    frame.push(JsValue::encode_f64_value((!n) as _));
                }
            }
            Opcode::OP_POS => {
                let value = frame.pop();
                if value.is_number() {
                    frame.push(value);
                }
                let x = value.to_number(rt)?;
                frame.push(JsValue::encode_f64_value(x));
            }

            Opcode::OP_DECL_CONST => {
                let val = frame.pop();
                let name = ip.cast::<u32>().read();
                ip = ip.add(8);
                let name = unwrap_unchecked(frame.code_block).names[name as usize];
                Env {
                    record: frame.env.get_jsobject(),
                }
                .declare_variable(rt, name, val, false)?;
            }
            Opcode::OP_DECL_LET => {
                let val = frame.pop();
                let name = ip.cast::<u32>().read();
                ip = ip.add(8);
                let name = unwrap_unchecked(frame.code_block).names[name as usize];
                Env {
                    record: frame.env.get_jsobject(),
                }
                .declare_variable(rt, name, val, true)?;
            }
            Opcode::OP_DELETE_VAR => {
                let name = ip.cast::<u32>().read();
                ip = ip.add(4);
                let name = unwrap_unchecked(frame.code_block).names[name as usize];
                let env = get_env(rt, frame, name);

                match env {
                    Some(mut env) => {
                        frame.push(JsValue::encode_bool_value(env.delete(rt, name, false)?))
                    }
                    None => {
                        frame.push(JsValue::encode_bool_value(true));
                    }
                }
            }
            Opcode::OP_DELETE_BY_ID => {
                let name = ip.cast::<u32>().read();
                ip = ip.add(4);
                let name = unwrap_unchecked(frame.code_block).names[name as usize];
                let object = frame.pop();
                object.check_object_coercible(rt)?;
                root!(object = gcstack, object.to_object(rt)?);
                frame.push(JsValue::new(object.delete(
                    rt,
                    name,
                    unwrap_unchecked(frame.code_block).strict,
                )?));
            }
            Opcode::OP_DELETE_BY_VAL => {
                let object = frame.pop();
                let name = frame.pop().to_symbol(rt)?;
                object.check_object_coercible(rt)?;
                root!(object = gcstack, object.to_object(rt)?);
                frame.push(JsValue::new(object.delete(
                    rt,
                    name,
                    unwrap_unchecked(frame.code_block).strict,
                )?));
            }
            Opcode::OP_GET_FUNCTION => {
                //vm.space().defer_gc();
                let ix = ip.cast::<u32>().read_unaligned();
                ip = ip.add(4);
                let func = JsVMFunction::new(
                    rt,
                    unwrap_unchecked(frame.code_block).codes[ix as usize],
                    (*frame).env.get_jsobject(),
                );
                assert!(func.is_callable());

                frame.push(JsValue::encode_object_value(func));
                // vm.space().undefer_gc();
            }

            Opcode::OP_PUSH_UNDEF => {
                frame.push(JsValue::encode_undefined_value());
            }
            Opcode::OP_NEWARRAY => {
                let count = ip.cast::<u32>().read_unaligned();

                ip = ip.add(4);
                root!(arr = gcstack, JsArray::new(rt, count));
                let mut index = 0;
                while index < count {
                    let value = frame.pop();
                    if unlikely(value.is_object() && value.get_object().is::<SpreadValue>()) {
                        root!(
                            spread = gcstack,
                            value.get_object().downcast_unchecked::<SpreadValue>()
                        );
                        for i in 0..spread.array.get(rt, "length".intern())?.get_number() as usize {
                            let real_arg = spread.array.get(rt, Symbol::Index(i as _))?;
                            arr.put(rt, Symbol::Index(index), real_arg, false)?;
                            index += 1;
                        }
                    } else {
                        arr.put(rt, Symbol::Index(index), value, false)?;
                        index += 1;
                    }
                }
                frame.push(JsValue::encode_object_value(*arr));
            }

            Opcode::OP_CALL_BUILTIN => {
                rt.gc().collect_if_necessary();
                let argc = ip.cast::<u32>().read();
                ip = ip.add(4);
                let builtin_id = ip.cast::<u32>().read();
                ip = ip.add(4);
                let effect = ip.cast::<u32>().read();
                ip = ip.add(4);
                super::builtins::BUILTINS[builtin_id as usize](
                    rt,
                    frame,
                    &mut ip,
                    argc,
                    effect as _,
                )?;
            }
            Opcode::OP_SPREAD => {
                /*
                    This opcode creates internal interpreter only value that is used to indicate that some argument is spread value
                    and if interpreter sees it then it tried to use `array` value from `SpreadValue`.
                    User code can't get access to this value, if it does this should be reported.

                */
                let value = frame.pop();
                let spread = SpreadValue::new(rt, value)?;
                frame.push(JsValue::encode_object_value(spread));
            }
            Opcode::OP_TYPEOF => {
                let val = frame.pop();
                let str = JsString::new(rt, val.type_of());
                frame.push(JsValue::new(str));
            }
            x => panic!("{:?}", x),
        }
    }
}
fn get_env(rt: &mut Runtime, frame: &mut CallFrame, name: Symbol) -> Option<GcPointer<JsObject>> {
    'search: loop {
        let mut current = Some((*frame).env.get_jsobject());
        while let Some(env) = current {
            if (Env { record: env }).has_own_variable(rt, name) {
                break 'search Some(env);
            }
            current = env.prototype().copied();
        }
        break 'search None;
    }
}
#[inline(never)]
pub unsafe fn get_var(
    rt: &mut Runtime,
    name: Symbol,
    frame: &mut CallFrame,
    fdbk: u32,
) -> Result<JsValue, JsValue> {
    let stack = rt.shadowstack();
    let env = get_env(rt, frame, name);
    root!(
        env = stack,
        match env {
            Some(env) => env,
            None => rt.global_object(),
        }
    );

    if let TypeFeedBack::PropertyCache { structure, offset } = unwrap_unchecked(frame.code_block)
        .feedback
        .get_unchecked(fdbk as usize)
    {
        if let Some(structure) = structure.upgrade() {
            if GcPointer::ptr_eq(&structure, &env.structure()) {
                return Ok(*env.direct(*offset as usize));
            }
        }
    }

    let mut slot = Slot::new();
    if likely(env.get_own_property_slot(rt, name, &mut slot)) {
        if slot.is_load_cacheable() {
            *unwrap_unchecked(frame.code_block)
                .feedback
                .get_unchecked_mut(fdbk as usize) = TypeFeedBack::PropertyCache {
                structure: rt.gc().make_weak(env.structure()),
                offset: slot.offset(),
            };
        }

        let value = slot.value();
        // println!("{}", value.is_callable());
        return Ok(value);
    };
    let msg = JsString::new(
        rt,
        format!("Undeclared variable '{}'", rt.description(name)),
    );
    Err(JsValue::encode_object_value(JsReferenceError::new(
        rt, msg, None,
    )))
}
#[inline(never)]
pub unsafe fn set_var(
    rt: &mut Runtime,
    frame: &mut CallFrame,
    name: Symbol,
    fdbk: u32,
    val: JsValue,
) -> Result<(), JsValue> {
    let stack = rt.shadowstack();
    let env = get_env(rt, frame, name);
    root!(
        env = stack,
        match env {
            Some(env) => env,
            None if likely(!unwrap_unchecked(frame.code_block).strict) => rt.global_object(),
            _ => {
                let msg = JsString::new(
                    rt,
                    format!("Unresolved reference '{}'", rt.description(name)),
                );
                return Err(JsValue::encode_object_value(JsReferenceError::new(
                    rt, msg, None,
                )));
            }
        }
    );
    if let TypeFeedBack::PropertyCache { structure, offset } = unwrap_unchecked(frame.code_block)
        .feedback
        .get_unchecked(fdbk as usize)
    {
        if let Some(structure) = structure.upgrade() {
            if likely(GcPointer::ptr_eq(&structure, &env.structure())) {
                *env.direct_mut(*offset as usize) = val;
            }
        }
    }

    let mut slot = Slot::new();
    if GcPointer::ptr_eq(&env, &rt.global_object()) {
        env.put(rt, name, val, unwrap_unchecked(frame.code_block).strict)?;
        return Ok(());
    }
    assert!(env.get_own_property_slot(rt, name, &mut slot));
    let slot = Env { record: *env }.set_variable(
        rt,
        name,
        val,
        unwrap_unchecked(frame.code_block).strict,
    )?;
    *unwrap_unchecked(frame.code_block)
        .feedback
        .get_unchecked_mut(fdbk as usize) = TypeFeedBack::PropertyCache {
        structure: rt.gc().make_weak(slot.0.structure()),
        offset: slot.1.offset(),
    };
    //*env.direct_mut(slot.1.offset() as usize) = val;
    Ok(())
}

/// Type used internally in JIT/interpreter to represent spread result.
pub struct SpreadValue {
    pub(crate) array: GcPointer<JsObject>,
}

impl SpreadValue {
    pub fn new(rt: &mut Runtime, value: JsValue) -> Result<GcPointer<Self>, JsValue> {
        unsafe {
            if value.is_jsobject() {
                if value.get_object().downcast_unchecked::<JsObject>().tag() == ObjectTag::Array {
                    return Ok(rt.gc().allocate(Self {
                        array: value.get_object().downcast_unchecked(),
                    }));
                }
            }

            let msg = JsString::new(rt, "cannot create spread from non-array value");
            Err(JsValue::encode_object_value(JsTypeError::new(
                rt, msg, None,
            )))
        }
    }
}

impl GcCell for SpreadValue {
    fn deser_pair(&self) -> (usize, usize) {
        (Self::deserialize as _, Self::allocate as _)
    }
    vtable_impl!();
}
unsafe impl Trace for SpreadValue {
    fn trace(&mut self, visitor: &mut dyn Tracer) {
        self.array.trace(visitor);
    }
}

pub fn get_by_id_slow(rt: &mut Runtime, name: Symbol, val: JsValue) -> Result<JsValue, JsValue> {
    let mut slot = Slot::new();
    val.get_slot(rt, name, &mut slot)
}
