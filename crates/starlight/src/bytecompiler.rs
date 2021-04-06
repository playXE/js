use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{
    bytecode::{opcodes::Opcode, TypeFeedBack},
    prelude::*,
    vm::{code_block::CodeBlock, RuntimeRef},
};
pub struct LoopControlInfo {
    breaks: Vec<Box<dyn FnOnce(&mut ByteCompiler)>>,
    continues: Vec<Box<dyn FnOnce(&mut ByteCompiler)>>,
    scope_depth: i32,
}
use super::codegen::BindingKind;
use super::codegen::Scope as Analyzer;
use swc_common::DUMMY_SP;
use swc_ecmascript::visit::Node;
use swc_ecmascript::visit::Visit;
use swc_ecmascript::visit::VisitWith;
use swc_ecmascript::{ast::*, visit::noop_visit_type};
pub type ScopeRef = Rc<RefCell<Scope>>;
/// JS variable scope representation at compile-time.
pub struct Scope {
    pub parent: Option<ScopeRef>,
    pub variables: HashMap<Symbol, Variable>,
    pub depth: u32,
}
impl Scope {
    pub fn add_var(&mut self, name: Symbol) -> u16 {
        let len = self.variables.len();
        self.variables.insert(
            name,
            Variable {
                kind: VariableKind::Var,
                name,
                index: len as u16,
            },
        );
        len as _
    }
}

pub struct Variable {
    pub name: Symbol,
    pub index: u16,
    pub kind: VariableKind,
}

pub enum VariableKind {
    Let,
    Const,
    Var,
    Global,
}

pub enum Access {
    Variable(u16, u32),
    Global(Symbol),
    ById(Symbol),
    ByVal,
    This,
}
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Val {
    Float(u64),
    Str(String),
}
pub struct ByteCompiler {
    pub code: GcPointer<CodeBlock>,
    pub scope: Rc<RefCell<Scope>>,
    pub rt: RuntimeRef,
    pub val_map: HashMap<Val, u32>,
    pub name_map: HashMap<Symbol, u32>,
    pub lci: Vec<LoopControlInfo>,
    pub fmap: HashMap<Symbol, u32>,
    pub top_level: bool,
}

impl ByteCompiler {
    pub fn get_val(&mut self, vm: &mut Runtime, val: Val) -> u32 {
        if let Some(ix) = self.val_map.get(&val) {
            return *ix;
        }

        let val_ = match val.clone() {
            Val::Float(x) => JsValue::new(f64::from_bits(x)),
            Val::Str(x) => JsValue::encode_object_value(JsString::new(vm, x)),
        };
        let ix = self.code.literals.len();
        self.code.literals.push(val_);
        self.val_map.insert(val, ix as _);
        ix as _
    }
    pub fn get_sym(&mut self, name: Symbol) -> u32 {
        if let Some(ix) = self.name_map.get(&name) {
            return *ix;
        }
        let ix = self.code.names.len();
        self.code.names.push(name);
        self.name_map.insert(name, ix as _);
        ix as _
    }
    fn lookup_scope(&self, var: Symbol) -> Option<(u16, ScopeRef)> {
        let scope = self.scope.clone();

        if let Some(var) = scope.borrow().variables.get(&var).map(|x| x.index) {
            return Some((var, scope.clone()));
        }
        let mut scope = self.scope.borrow().parent.clone();
        while let Some(ns) = scope {
            if let Some(var) = ns.borrow().variables.get(&var).map(|x| x.index) {
                return Some((var, ns.clone()));
            }
            scope = ns.borrow().parent.clone();
        }
        None
    }

    fn access_var(&self, var: Symbol) -> Access {
        if let Some((ix, scope)) = self.lookup_scope(var) {
            let cur_depth = self.scope.borrow().depth;
            let depth = cur_depth - scope.borrow().depth;
            Access::Variable(ix, depth)
        } else {
            Access::Global(var)
        }
    }

    pub fn decl_const(&mut self, name: Symbol) -> u16 {
        self.emit(Opcode::OP_GET_ENV, &[0], false);
        let _ix = self.scope.borrow_mut().add_var(name);
        self.emit(Opcode::OP_DECL_CONST, &[], false);

        _ix
    }

    pub fn decl_let(&mut self, name: Symbol) -> u16 {
        self.emit(Opcode::OP_GET_ENV, &[0], false);
        let ix = self.scope.borrow_mut().add_var(name);
        self.emit(Opcode::OP_DECL_LET, &[], false);
        //self.emit_u16(ix);
        ix
    }

    pub fn ident_to_sym(id: &Ident) -> Symbol {
        let s: &str = &id.sym;
        s.intern()
    }
    pub fn var_decl(&mut self, var: &VarDecl) -> Vec<Symbol> {
        let mut names = vec![];
        for decl in var.decls.iter() {
            match &decl.name {
                Pat::Ident(name) => {
                    match &decl.init {
                        Some(ref init) => {
                            self.expr(init, true);
                        }
                        None => {
                            self.emit(Opcode::OP_PUSH_UNDEF, &[], false);
                        }
                    }
                    names.push(Self::ident_to_sym(&name.id));
                    match var.kind {
                        VarDeclKind::Const => {
                            self.decl_const(Self::ident_to_sym(&name.id));
                        }
                        VarDeclKind::Let => {
                            self.decl_let(Self::ident_to_sym(&name.id));
                        }
                        VarDeclKind::Var => {
                            let acc = self.access_var(Self::ident_to_sym(&name.id));
                            self.access_set(acc);
                        }
                    }
                }
                _ => todo!(),
            }
        }
        names
    }
    pub fn access_delete(&mut self, acc: Access) {
        match acc {
            Access::Global(x) => {
                let id = self.get_sym(x);
                self.emit(Opcode::OP_GLOBALTHIS, &[], false);
                self.emit(Opcode::OP_DELETE_BY_ID, &[id], false);
            }
            Access::ByVal => {
                self.emit(Opcode::OP_DELETE_BY_VAL, &[], false);
            }
            Access::ById(x) => {
                let id = self.get_sym(x);
                self.emit(Opcode::OP_DELETE_BY_ID, &[id], false);
            }
            Access::Variable(_ix, _depth) => {
                self.emit(Opcode::OP_PUSH_TRUE, &[], false);
                // self.access_set()
            }
            _ => unreachable!(),
        }
    }
    pub fn access_set(&mut self, acc: Access) {
        match acc {
            Access::Variable(index, depth) => {
                self.emit(Opcode::OP_GET_ENV, &[depth], false);
                self.emit(Opcode::OP_SET_VAR, &[index as _], false);
                //self.emit_u16(index);
            }
            Access::Global(x) => {
                let name = self.get_sym(x);
                self.emit(Opcode::OP_GLOBALTHIS, &[], false);
                self.emit(Opcode::OP_PUT_BY_ID, &[name], true);
            }
            Access::ById(name) => {
                let name = self.get_sym(name);
                self.emit(Opcode::OP_PUT_BY_ID, &[name], true);
            }
            Access::ByVal => self.emit(Opcode::OP_PUT_BY_VAL, &[], false),
            _ => todo!(),
        }
    }
    pub fn access_get(&mut self, acc: Access) {
        match acc {
            Access::Variable(index, depth) => {
                self.emit(Opcode::OP_GET_ENV, &[depth], false);
                self.emit(Opcode::OP_GET_VAR, &[index as _], false);
                //self.emit_u16(index);
            }
            Access::Global(x) => {
                let name = self.get_sym(x);
                self.emit(Opcode::OP_GLOBALTHIS, &[], false);
                self.emit(Opcode::OP_TRY_GET_BY_ID, &[name], true);
            }
            Access::ById(name) => {
                let name = self.get_sym(name);
                self.emit(Opcode::OP_GET_BY_ID, &[name], true);
            }
            Access::ByVal => self.emit(Opcode::OP_GET_BY_VAL, &[], false),
            _ => todo!(),
        }
    }

    pub fn compile_access(&mut self, expr: &Expr, dup: bool) -> Access {
        match expr {
            Expr::Ident(id) => self.access_var(Self::ident_to_sym(id)),
            Expr::Member(member) => {
                match &member.obj {
                    ExprOrSuper::Expr(e) => self.expr(e, true),
                    _ => todo!(),
                }
                if dup {
                    self.emit(Opcode::OP_DUP, &[], false);
                }
                let name = if member.computed {
                    None
                } else {
                    if let Expr::Ident(name) = &*member.prop {
                        Some(Self::ident_to_sym(name))
                    } else {
                        None
                    }
                };
                if name.is_none() {
                    self.expr(&member.prop, true);
                    self.emit(Opcode::OP_SWAP, &[], false);
                }

                if let Some(name) = name {
                    Access::ById(name)
                } else {
                    Access::ByVal
                }
            }
            Expr::This(_) => Access::This,
            _ => todo!(),
        }
    }
    pub fn finish(&mut self, vm: &mut Runtime) -> GcPointer<CodeBlock> {
        if vm.options.dump_bytecode {
            let mut buf = String::new();
            let name = vm.description(self.code.name);
            self.code.display_to(&mut buf).unwrap();
            eprintln!("Code block '{}' at {:p}: \n {}", name, self.code, buf);
        }
        self.code
    }
    pub fn compile_fn(&mut self, fun: &Function) {
        #[cfg(feature = "perf")]
        {
            self.vm.perf.set_prev_inst(crate::vm::perf::Perf::CODEGEN);
        }
        let is_strict = match fun.body {
            Some(ref body) => {
                if body.stmts.is_empty() {
                    false
                } else {
                    body.stmts[0].is_use_strict()
                }
            }
            None => false,
        };
        self.code.strict = is_strict;

        match fun.body {
            Some(ref body) => {
                self.compile(&body.stmts);
            }
            None => {}
        }
        //self.emit(Opcode::OP_PUSH_UNDEFINED, &[], false);
        self.emit(Opcode::OP_RET, &[], false);
        //self.finish(&mut self.vm);
        #[cfg(feature = "perf")]
        {
            self.vm.perf.get_perf(crate::vm::perf::Perf::INVALID);
        }
    }
    pub fn compile_script(mut vm: &mut Runtime, p: &Script) -> GcPointer<CodeBlock> {
        let name = "<script>".intern();
        let mut code = CodeBlock::new(&mut vm, name, false);

        let mut compiler = ByteCompiler {
            lci: Vec::new(),
            top_level: true,
            scope: Rc::new(RefCell::new(Scope {
                parent: None,
                variables: Default::default(),
                depth: 0,
            })),
            code: code,
            val_map: Default::default(),
            name_map: Default::default(),
            fmap: Default::default(),
            rt: RuntimeRef(vm),
        };

        let is_strict = match p.body.get(0) {
            Some(ref body) => body.is_use_strict(),
            None => false,
        };
        code.top_level = true;
        code.strict = is_strict;
        compiler.compile(&p.body);
        // compiler.builder.emit(Opcode::OP_PUSH_UNDEFINED, &[], false);
        compiler.emit(Opcode::OP_RET, &[], false);
        let mut rt = compiler.rt;
        let result = compiler.finish(&mut rt);

        result
    }
    pub fn compile(&mut self, body: &[Stmt]) {
        let scopea = Analyzer::analyze_stmts(body);
        if !self.top_level {
            for var in scopea.vars.iter() {
                match var.1.kind() {
                    BindingKind::Var => {
                        let s: &str = &(var.0).0;
                        let name = s.intern();
                        self.scope.borrow_mut().add_var(name);
                        self.code.var_count += 1;
                    }
                    BindingKind::Function => {
                        let s: &str = &(var.0).0;
                        let name = s.intern();
                        self.scope.borrow_mut().add_var(name);
                        self.code.var_count += 1;
                    }
                    _ => (),
                }
            }
        }
        VisitFnDecl::visit(body, &mut |decl| {
            let name = Self::ident_to_sym(&decl.ident);
            let mut _rest = None;
            let mut params = vec![];
            let mut rat = None;
            let mut code = CodeBlock::new(&mut self.rt, name, false);
            let scope = Rc::new(RefCell::new(Scope {
                variables: HashMap::new(),
                parent: Some(self.scope.clone()),
                depth: self.scope.borrow().depth + 1,
            }));

            let mut compiler = ByteCompiler {
                lci: Vec::new(),
                code,
                fmap: HashMap::new(),
                val_map: HashMap::new(),
                name_map: HashMap::new(),
                top_level: false,
                scope,
                rt: RuntimeRef(&mut *self.rt),
            };
            for x in decl.function.params.iter() {
                match x.pat {
                    Pat::Ident(ref x) => {
                        params.push(Self::ident_to_sym(&x.id));
                        compiler
                            .scope
                            .borrow_mut()
                            .add_var(Self::ident_to_sym(&x.id));
                    }
                    Pat::Rest(ref r) => match &*r.arg {
                        Pat::Ident(ref id) => {
                            _rest = Some(Self::ident_to_sym(&id.id));
                            rat = Some(
                                compiler
                                    .scope
                                    .borrow_mut()
                                    .add_var(Self::ident_to_sym(&id.id))
                                    as u32,
                            );
                        }
                        _ => unreachable!(),
                    },
                    _ => todo!(),
                }
            }

            code.param_count = params.len() as _;
            code.rest_at = rat;
            compiler.compile_fn(&decl.function);
            let ix = self.code.codes.len();
            let code = compiler.finish(&mut self.rt);
            self.code.codes.push(code);
            self.fmap.insert(name, ix as _);
            self.emit(Opcode::OP_GET_FUNCTION, &[ix as _], false);
            let var = self.access_var(name);
            self.access_set(var);
        });

        for stmt in body.iter() {
            if contains_ident(stmt, "arguments") {
                self.code.use_arguments = true;
                self.code.args_at = self.scope.borrow_mut().add_var("arguments".intern()) as _;
                break;
            }
        }

        for stmt in body {
            self.stmt(stmt);
        }
    }

    /// Push scope and return current scope depth
    pub fn push_scope(&mut self) -> u32 {
        let d = self.scope.borrow().depth;
        let new_scope = Rc::new(RefCell::new(Scope {
            parent: Some(self.scope.clone()),
            depth: self.scope.borrow().depth + 1,
            variables: Default::default(),
        }));
        self.scope = new_scope;
        d
    }
    pub fn pop_scope(&mut self) {
        let scope = self.scope.clone();
        self.scope = scope.borrow().parent.clone().expect("No scopes left");
    }
    pub fn push_lci(&mut self, _continue_target: u32, depth: u32) {
        self.lci.push(LoopControlInfo {
            continues: vec![],
            breaks: vec![],
            scope_depth: depth as _,
        });
    }

    pub fn pop_lci(&mut self) {
        let mut lci = self.lci.pop().unwrap();
        while let Some(break_) = lci.breaks.pop() {
            break_(self);
        }
    }
    pub fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(expr) => {
                self.expr(&expr.expr, false);
            }
            Stmt::Block(block) => {
                let _prev = self.push_scope();
                self.emit(Opcode::OP_PUSH_ENV, &[], false);
                for stmt in block.stmts.iter() {
                    self.stmt(stmt);
                }
                self.pop_scope();
                self.emit(Opcode::OP_POP_ENV, &[], false);
                //self.emit(Opcode::OP_SET_ENV, &[prev], false);
            }
            Stmt::Return(ret) => {
                match ret.arg {
                    Some(ref arg) => self.expr(arg, true),
                    None => self.emit(Opcode::OP_PUSH_UNDEF, &[], false),
                };
                self.emit(Opcode::OP_RET, &[], false);
            }
            Stmt::Break(_) => {
                let depth = self.lci.last().unwrap().scope_depth;
                assert!(depth >= 0);
                let to = self.scope.borrow().depth - depth as u32;
                self.emit(Opcode::OP_SET_ENV, &[to], false);
                let br = self.jmp();
                self.lci.last_mut().unwrap().breaks.push(Box::new(br));
            }
            Stmt::Continue(_) => {
                let depth = self.lci.last().unwrap().scope_depth;
                assert!(depth > 0);
                let to = self.scope.borrow().depth - depth as u32;
                self.emit(Opcode::OP_SET_ENV, &[to], false);
                // self.emit(Opcode::OP_POP_ENV, &[], false);
                let j = self.jmp();
                self.lci.last_mut().unwrap().continues.push(Box::new(j));
            }
            Stmt::ForIn(for_in) => {
                let depth = self.push_scope();
                self.emit(Opcode::OP_PUSH_ENV, &[], false);
                let name = match for_in.left {
                    VarDeclOrPat::VarDecl(ref var_decl) => self.var_decl(var_decl)[0],
                    VarDeclOrPat::Pat(Pat::Ident(ref ident)) => {
                        let sym = Self::ident_to_sym(&ident.id);
                        self.emit(Opcode::OP_PUSH_UNDEF, &[], false);
                        self.emit(Opcode::OP_GET_ENV, &[0], false);
                        self.decl_let(sym);
                        sym
                    }
                    _ => unreachable!(),
                };

                self.expr(&for_in.right, true);
                let for_in_setup = self.jmp_custom(Opcode::OP_FORIN_SETUP);
                let head = self.code.code.len();
                self.push_lci(head as _, depth);
                let for_in_enumerate = self.jmp_custom(Opcode::OP_FORIN_ENUMERATE);
                let acc = self.access_var(name);
                self.access_set(acc);
                //self.emit(Opcode::OP_SET_VAR, &[name], true);
                self.stmt(&for_in.body);
                while let Some(c) = self.lci.last_mut().unwrap().continues.pop() {
                    c(self);
                }

                self.goto(head as _);

                for_in_enumerate(self);
                for_in_setup(self);
                self.emit(Opcode::OP_POP_ENV, &[], false);
                self.pop_scope();
                self.emit(Opcode::OP_FORIN_LEAVE, &[], false);
            }
            Stmt::For(for_stmt) => {
                let _env = self.push_scope();
                self.emit(Opcode::OP_PUSH_ENV, &[], false);
                match for_stmt.init {
                    Some(ref init) => match init {
                        VarDeclOrExpr::Expr(ref e) => {
                            self.expr(e, false);
                        }
                        VarDeclOrExpr::VarDecl(ref decl) => {
                            self.var_decl(decl);
                        }
                    },
                    None => {}
                }

                let head = self.code.code.len();
                self.push_lci(head as _, _env);
                match for_stmt.test {
                    Some(ref test) => {
                        self.expr(&**test, true);
                    }
                    None => {
                        self.emit(Opcode::OP_PUSH_TRUE, &[], false);
                    }
                }
                let jend = self.cjmp(false);
                self.stmt(&for_stmt.body);
                let skip = self.jmp();
                while let Some(c) = self.lci.last_mut().unwrap().continues.pop() {
                    c(self);
                }

                //self.emit(Opcode::OP_POP_ENV, &[], false);
                skip(self);
                if let Some(fin) = &for_stmt.update {
                    self.expr(&**fin, false);
                }
                self.goto(head as _);
                self.pop_lci();
                self.pop_scope();
                // self.emit(Opcode::OP_POP_ENV, &[], false);
                jend(self);

                self.emit(Opcode::OP_POP_ENV, &[], false);
            }
            Stmt::While(while_stmt) => {
                let head = self.code.code.len();
                let d = self.scope.borrow().depth;
                self.push_lci(head as _, d);
                self.expr(&while_stmt.test, true);
                let jend = self.cjmp(false);
                self.stmt(&while_stmt.body);

                while let Some(c) = self.lci.last_mut().unwrap().continues.pop() {
                    c(self);
                }
                self.goto(head);
                jend(self);
                self.pop_lci();
            }
            Stmt::If(if_stmt) => {
                self.expr(&if_stmt.test, true);
                let jelse = self.cjmp(false);
                self.stmt(&if_stmt.cons);
                match if_stmt.alt {
                    None => {
                        jelse(self);
                    }
                    Some(ref alt) => {
                        let jend = self.jmp();
                        jelse(self);
                        self.stmt(&**alt);
                        jend(self);
                    }
                }
            }
            Stmt::Decl(decl) => match decl {
                Decl::Var(var) => {
                    self.var_decl(var);
                }
                Decl::Fn(fun) => {
                    let s: &str = &fun.ident.sym;
                    let sym = s.intern();
                    let ix = *self.fmap.get(&sym).unwrap();
                    self.emit(Opcode::OP_GET_FUNCTION, &[ix], false);
                    let var = self.access_var(sym);
                    self.access_set(var);
                    // self.emit(Opcode::OP_SET_VAR, &[nix], true);
                }
                _ => (),
            },

            Stmt::Empty(_) => {}
            Stmt::Throw(throw) => {
                self.expr(&throw.arg, true);
                self.emit(Opcode::OP_THROW, &[], false);
            }
            Stmt::Try(try_stmt) => {
                let try_push = self.try_();
                if !try_stmt.block.stmts.is_empty() {
                    if let Some(lci) = self.lci.last_mut() {
                        lci.scope_depth += 1;
                    }
                    self.emit(Opcode::OP_PUSH_ENV, &[], false);
                }
                for stmt in try_stmt.block.stmts.iter() {
                    self.stmt(stmt);
                }
                if !try_stmt.block.stmts.is_empty() {
                    self.emit(Opcode::OP_POP_ENV, &[], false);
                }
                let jfinally = self.jmp();
                try_push(self);
                let jcatch_finally = match try_stmt.handler {
                    Some(ref catch) => {
                        self.push_scope();
                        if !catch.body.stmts.is_empty() {
                            self.emit(Opcode::OP_PUSH_ENV, &[], false);
                        }
                        match catch.param {
                            Some(ref pat) => {
                                let acc = self.compile_access_pat(pat, false);
                                self.access_set(acc);

                                //self.generate_pat_store(pat, true, true);
                            }
                            None => {
                                self.emit(Opcode::OP_POP, &[], false);
                            }
                        }
                        for stmt in catch.body.stmts.iter() {
                            self.stmt(stmt);
                        }
                        if !catch.body.stmts.is_empty() {
                            self.emit(Opcode::OP_POP_ENV, &[], false);
                        }
                        self.pop_scope();
                        self.jmp()
                    }
                    None => {
                        self.emit(Opcode::OP_POP, &[], false);
                        self.jmp()
                    }
                };

                jfinally(self);
                jcatch_finally(self);
                match try_stmt.finalizer {
                    Some(ref block) => {
                        self.push_scope();
                        if !block.stmts.is_empty() {
                            self.emit(Opcode::OP_PUSH_ENV, &[], false);
                        }
                        for stmt in block.stmts.iter() {
                            self.stmt(stmt);
                        }
                        if !block.stmts.is_empty() {
                            self.emit(Opcode::OP_POP_ENV, &[], false);
                        }
                        self.pop_scope();
                    }
                    None => {}
                }
            }

            x => todo!("{:?}", x),
        }
    }
    pub fn compile_access_pat(&mut self, pat: &Pat, dup: bool) -> Access {
        match pat {
            Pat::Ident(id) => self.access_var(Self::ident_to_sym(&id.id)),
            Pat::Expr(expr) => self.compile_access(expr, dup),
            _ => todo!(),
        }
    }

    pub fn expr(&mut self, expr: &Expr, used: bool) {
        match expr {
            Expr::Ident(id) => {
                if &id.sym == "undefined" {
                    self.emit(Opcode::OP_PUSH_UNDEF, &[], false);
                } else {
                    let var = self.access_var(Self::ident_to_sym(id));
                    self.access_get(var);
                }
                if !used {
                    self.emit(Opcode::OP_POP, &[], false);
                }
            }
            Expr::Lit(lit) => {
                match lit {
                    Lit::Bool(x) => {
                        if x.value {
                            self.emit(Opcode::OP_PUSH_TRUE, &[], false)
                        } else {
                            self.emit(Opcode::OP_PUSH_FALSE, &[], false)
                        }
                    }
                    Lit::Null(_) => self.emit(Opcode::OP_PUSH_NULL, &[], false),
                    Lit::Num(num) => {
                        if num.value as i32 as f64 == num.value {
                            self.emit(Opcode::OP_PUSH_INT, &[num.value as i32 as u32], false)
                        } else {
                            let mut vm = self.rt;
                            let ix = self.get_val(&mut vm, Val::Float(num.value.to_bits()));
                            self.emit(Opcode::OP_PUSH_LITERAL, &[ix], false);
                        }
                    }
                    Lit::Str(str) => {
                        let mut vm = self.rt;
                        let str = self.get_val(&mut vm, Val::Str(str.value.to_string()));
                        self.emit(Opcode::OP_PUSH_LITERAL, &[str], false);
                    }
                    _ => todo!(),
                }
                if !used {
                    self.emit(Opcode::OP_POP, &[], false);
                }
            }
            Expr::This(_) => {
                if used {
                    self.emit(Opcode::OP_PUSH_THIS, &[], false);
                }
            }
            Expr::Member(_) => {
                let acc = self.compile_access(expr, false);
                self.access_get(acc);
                if !used {
                    self.emit(Opcode::OP_POP, &[], false);
                }
            }
            Expr::Object(object_lit) => {
                self.emit(Opcode::OP_NEWOBJECT, &[], false);
                for prop in object_lit.props.iter() {
                    match prop {
                        PropOrSpread::Prop(prop) => match &**prop {
                            Prop::Shorthand(ident) => {
                                self.emit(Opcode::OP_DUP, &[], false);
                                let ix = Self::ident_to_sym(ident);
                                let acc = self.access_var(ix);
                                let sym = self.get_sym(ix);
                                self.access_get(acc);
                                // self.emit(Opcode::OP_GET_VAR, &[sym], true);
                                self.emit(Opcode::OP_SWAP, &[], false);
                                self.emit(Opcode::OP_PUT_BY_ID, &[sym], true);
                            }
                            Prop::KeyValue(assign) => {
                                self.emit(Opcode::OP_DUP, &[], false);
                                self.expr(&assign.value, true);
                                let mut rt = self.rt;
                                match assign.key {
                                    PropName::Ident(ref id) => {
                                        let ix = Self::ident_to_sym(id);
                                        let sym = self.get_sym(ix);
                                        self.emit(Opcode::OP_SWAP, &[], false);
                                        self.emit(Opcode::OP_PUT_BY_ID, &[sym], true);
                                    }
                                    PropName::Str(ref s) => {
                                        let ix =
                                            self.get_val(&mut rt, Val::Str(s.value.to_string()));
                                        self.emit(Opcode::OP_SWAP, &[], false);
                                        self.emit(Opcode::OP_PUSH_LITERAL, &[ix], false);
                                        self.emit(Opcode::OP_SWAP, &[], false);
                                        self.emit(Opcode::OP_PUT_BY_VAL, &[], false);
                                    }
                                    PropName::Num(n) => {
                                        let val = n.value;
                                        if val as i32 as f64 == val {
                                            self.emit(Opcode::OP_SWAP, &[], false);
                                            self.emit(
                                                Opcode::OP_PUSH_INT,
                                                &[val as i32 as u32],
                                                false,
                                            );
                                            self.emit(Opcode::OP_SWAP, &[], false);
                                            self.emit(Opcode::OP_PUT_BY_VAL, &[], false);
                                        } else {
                                            let ix =
                                                self.get_val(&mut rt, Val::Float(val.to_bits()));
                                            self.emit(Opcode::OP_SWAP, &[], false);
                                            self.emit(Opcode::OP_PUSH_LITERAL, &[ix], false);
                                            self.emit(Opcode::OP_SWAP, &[], false);
                                            self.emit(Opcode::OP_PUT_BY_VAL, &[], false);
                                        }
                                    }
                                    _ => todo!(),
                                }
                            }
                            p => todo!("{:?}", p),
                        },
                        _ => todo!(),
                    }
                }
            }

            Expr::Call(call) => {
                // self.emit(Opcode::OP_PUSH_EMPTY, &[], false);
                let has_spread = call.args.iter().any(|x| x.spread.is_some());
                if has_spread {
                    for arg in call.args.iter().rev() {
                        self.expr(&arg.expr, true);
                        if arg.spread.is_some() {
                            self.emit(Opcode::OP_SPREAD, &[], false);
                        }
                    }
                    self.emit(Opcode::OP_NEWARRAY, &[call.args.len() as u32], false);
                } else {
                    for arg in call.args.iter() {
                        self.expr(&arg.expr, true);
                        assert!(arg.spread.is_none());
                    }
                }

                match call.callee {
                    ExprOrSuper::Super(_) => todo!(), // todo super call
                    ExprOrSuper::Expr(ref expr) => match &**expr {
                        Expr::Member(member) => {
                            let name = if let Expr::Ident(id) = &*member.prop {
                                let s: &str = &id.sym;
                                let name = s.intern();
                                self.get_sym(name)
                            } else {
                                unreachable!()
                            };
                            match member.obj {
                                ExprOrSuper::Expr(ref expr) => {
                                    self.expr(expr, true);
                                    self.emit(Opcode::OP_DUP, &[], false);
                                }
                                ExprOrSuper::Super(_super) => {
                                    todo!()
                                }
                            }

                            self.emit(Opcode::OP_GET_BY_ID, &[name], true);
                        }
                        _ => {
                            self.emit(Opcode::OP_PUSH_UNDEF, &[], false);
                            self.expr(&**expr, true);
                        }
                    },
                }
                if !has_spread {
                    self.emit(Opcode::OP_CALL, &[call.args.len() as u32], false);
                } else {
                    self.emit(
                        Opcode::OP_CALL_BUILTIN,
                        &[call.args.len() as _, 0, 0],
                        false,
                    );
                }
                if !used {
                    self.emit(Opcode::OP_POP, &[], false);
                }
            }
            Expr::Unary(unary) => {
                if let UnaryOp::Delete = unary.op {
                    let acc = self.compile_access(&*unary.arg, false);
                    self.access_delete(acc);
                    /* match &*unary.arg {
                        Expr::Member(member) => {
                            let name = if !member.computed {
                                if let Expr::Ident(id) = &*member.prop {
                                    let s: &str = &id.sym;
                                    let name = s.intern();
                                    Some(self.get_sym(name))
                                } else {
                                    self.expr(&member.prop, true);
                                    None
                                }
                            } else {
                                self.expr(&member.prop, true);
                                None
                            };

                            match member.obj {
                                ExprOrSuper::Expr(ref e) => self.expr(e, true),
                                _ => todo!(),
                            }

                            if let Some(name) = name {
                                self.emit(Opcode::OP_DELETE_BY_ID, &[name], false);
                            } else {
                                self.emit(Opcode::OP_DELETE_BY_VAL, &[], false);
                            }
                            return;
                        }
                        Expr::Ident(x) => {}
                        _ => todo!(),
                    }*/
                    return;
                }
                self.expr(&unary.arg, true);
                match unary.op {
                    UnaryOp::Minus => self.emit(Opcode::OP_NEG, &[], false),
                    UnaryOp::Plus => self.emit(Opcode::OP_POS, &[], false),
                    UnaryOp::Tilde => self.emit(Opcode::OP_NOT, &[], false),
                    UnaryOp::Bang => self.emit(Opcode::OP_LOGICAL_NOT, &[], false),
                    UnaryOp::TypeOf => self.emit(Opcode::OP_TYPEOF, &[], false),

                    _ => todo!("{:?}", unary.op),
                }
                if !used {
                    self.emit(Opcode::OP_POP, &[], false)
                }
            }
            Expr::Update(update) => {
                let op = match update.op {
                    UpdateOp::PlusPlus => Opcode::OP_ADD,
                    UpdateOp::MinusMinus => Opcode::OP_SUB,
                };
                if update.prefix {
                    self.expr(&update.arg, true);
                    self.emit(Opcode::OP_PUSH_INT, &[1i32 as u32], false);
                    self.emit(op, &[], false);
                    if used {
                        self.emit(Opcode::OP_DUP, &[], false);
                    }
                    let acc = self.compile_access(&update.arg, false);
                    self.access_set(acc);
                    //self.emit_store_expr(&update.arg);
                } else {
                    self.expr(&update.arg, true);
                    if used {
                        self.emit(Opcode::OP_DUP, &[], false);
                    }
                    self.emit(Opcode::OP_PUSH_INT, &[1i32 as u32], false);
                    self.emit(op, &[], false);
                    let acc = self.compile_access(&update.arg, false);
                    self.access_set(acc);
                    //self.emit_store_expr(&update.arg);
                }
            }
            Expr::New(call) => {
                let argc = call.args.as_ref().map(|x| x.len() as u32).unwrap_or(0);
                let has_spread = if let Some(ref args) = call.args {
                    args.iter().any(|x| x.spread.is_some())
                } else {
                    false
                };
                if let Some(ref args) = call.args {
                    if has_spread {
                        for arg in args.iter().rev() {
                            self.expr(&arg.expr, true);
                            if arg.spread.is_some() {
                                self.emit(Opcode::OP_SPREAD, &[], false);
                            }
                        }
                        self.emit(Opcode::OP_NEWARRAY, &[argc], false);
                    } else {
                        for arg in args.iter() {
                            self.expr(&arg.expr, true);
                            assert!(arg.spread.is_none());
                        }
                    }
                }

                self.emit(Opcode::OP_PUSH_UNDEF, &[], false);
                self.expr(&*call.callee, true);
                if !has_spread {
                    self.emit(Opcode::OP_NEW, &[argc], false);
                } else {
                    self.emit(Opcode::OP_CALL_BUILTIN, &[argc as _, 0, 1], false);
                }
                if !used {
                    self.emit(Opcode::OP_POP, &[], false);
                }
            }
            Expr::Assign(assign) => {
                if let AssignOp::Assign = assign.op {
                    self.expr(&assign.right, true);
                    if used {
                        self.emit(Opcode::OP_DUP, &[], false);
                    }
                    let acc = match &assign.left {
                        PatOrExpr::Expr(expr) => self.compile_access(expr, false),
                        PatOrExpr::Pat(p) => self.compile_access_pat(p, false),
                    };

                    self.access_set(acc);
                } else {
                    self.expr(&assign.right, true);
                    let left = match &assign.left {
                        PatOrExpr::Expr(e) => self.compile_access(e, false),
                        PatOrExpr::Pat(p) => self.compile_access_pat(p, false),
                    };
                    self.access_get(left);

                    let op = match assign.op {
                        AssignOp::AddAssign => Opcode::OP_ADD,
                        AssignOp::SubAssign => Opcode::OP_SUB,
                        AssignOp::MulAssign => Opcode::OP_MUL,
                        AssignOp::DivAssign => Opcode::OP_DIV,
                        AssignOp::BitAndAssign => Opcode::OP_AND,
                        AssignOp::BitOrAssign => Opcode::OP_OR,
                        AssignOp::BitXorAssign => Opcode::OP_XOR,
                        AssignOp::ModAssign => Opcode::OP_REM,

                        _ => todo!(),
                    };
                    self.emit(op, &[], false);
                    if used {
                        self.emit(Opcode::OP_DUP, &[], false);
                    }
                    let left = match &assign.left {
                        PatOrExpr::Expr(e) => self.compile_access(e, false),
                        PatOrExpr::Pat(p) => self.compile_access_pat(p, false),
                    };
                    self.access_set(left);
                }
            }
            Expr::Bin(binary) => {
                match binary.op {
                    BinaryOp::LogicalOr => {
                        self.expr(&binary.left, true);
                        self.emit(Opcode::OP_DUP, &[], false);
                        let jtrue = self.cjmp(true);
                        self.emit(Opcode::OP_POP, &[], false);
                        self.expr(&binary.right, true);
                        //let end = self.jmp();
                        jtrue(self);
                        // self.emit(Opcode::OP_PUSH_TRUE, &[], false);
                        //end(self);
                        if !used {
                            self.emit(Opcode::OP_POP, &[], false);
                        }
                        return;
                    }
                    BinaryOp::LogicalAnd => {
                        self.expr(&binary.left, true);
                        self.emit(Opcode::OP_DUP, &[], false);
                        let jfalse = self.cjmp(false);
                        self.emit(Opcode::OP_POP, &[], false);
                        self.expr(&binary.right, true);
                        let end = self.jmp();
                        jfalse(self);
                        end(self);
                        if !used {
                            self.emit(Opcode::OP_POP, &[], false);
                        }
                        return;
                    }

                    _ => (),
                }
                self.expr(&binary.right, true);
                self.expr(&binary.left, true);

                match binary.op {
                    BinaryOp::Add => {
                        self.emit(Opcode::OP_ADD, &[], false);
                    }
                    BinaryOp::Sub => {
                        self.emit(Opcode::OP_SUB, &[], false);
                    }
                    BinaryOp::Mul => {
                        self.emit(Opcode::OP_MUL, &[], false);
                    }
                    BinaryOp::Div => {
                        self.emit(Opcode::OP_DIV, &[], false);
                    }
                    BinaryOp::Mod => self.emit(Opcode::OP_REM, &[], false),
                    BinaryOp::BitAnd => self.emit(Opcode::OP_AND, &[], false),
                    BinaryOp::BitOr => self.emit(Opcode::OP_OR, &[], false),
                    BinaryOp::BitXor => self.emit(Opcode::OP_XOR, &[], false),
                    BinaryOp::LShift => self.emit(Opcode::OP_SHL, &[], false),
                    BinaryOp::RShift => self.emit(Opcode::OP_SHR, &[], false),
                    BinaryOp::ZeroFillRShift => self.emit(Opcode::OP_USHR, &[], false),
                    BinaryOp::EqEq => {
                        self.emit(Opcode::OP_EQ, &[], false);
                    }
                    BinaryOp::EqEqEq => self.emit(Opcode::OP_STRICTEQ, &[], false),
                    BinaryOp::NotEq => self.emit(Opcode::OP_NEQ, &[], false),
                    BinaryOp::NotEqEq => self.emit(Opcode::OP_NSTRICTEQ, &[], false),
                    BinaryOp::Gt => self.emit(Opcode::OP_GREATER, &[], false),
                    BinaryOp::GtEq => self.emit(Opcode::OP_GREATEREQ, &[], false),
                    BinaryOp::Lt => self.emit(Opcode::OP_LESS, &[], false),
                    BinaryOp::LtEq => self.emit(Opcode::OP_LESSEQ, &[], false),
                    BinaryOp::In => self.emit(Opcode::OP_IN, &[], false),

                    _ => todo!(),
                }

                if !used {
                    self.emit(Opcode::OP_POP, &[], false);
                }
            }
            Expr::Arrow(fun) => {
                let is_strict = match &fun.body {
                    BlockStmtOrExpr::BlockStmt(block) => {
                        if block.stmts.is_empty() {
                            false
                        } else {
                            block.stmts[0].is_use_strict()
                        }
                    }
                    _ => false,
                };
                let name = "<anonymous>".intern();
                let mut code = CodeBlock::new(&mut self.rt, name, false);

                let mut compiler = ByteCompiler {
                    lci: Vec::new(),
                    top_level: false,

                    code: code,
                    val_map: Default::default(),
                    name_map: Default::default(),

                    fmap: Default::default(),
                    rt: RuntimeRef(&mut *self.rt),
                    scope: Rc::new(RefCell::new(Scope {
                        parent: Some(self.scope.clone()),
                        depth: self.scope.borrow().depth + 1,
                        variables: HashMap::new(),
                    })),
                };
                code.strict = is_strict;
                let mut params = vec![];
                let mut rest_at = None;
                for x in fun.params.iter() {
                    match x {
                        Pat::Ident(ref x) => {
                            params.push(Self::ident_to_sym(&x.id));
                            compiler
                                .scope
                                .borrow_mut()
                                .add_var(Self::ident_to_sym(&x.id));
                        }
                        Pat::Rest(ref r) => match &*r.arg {
                            Pat::Ident(ref id) => {
                                rest_at = Some(
                                    compiler
                                        .scope
                                        .borrow_mut()
                                        .add_var(Self::ident_to_sym(&id.id))
                                        as u32,
                                );
                            }
                            _ => unreachable!(),
                        },
                        _ => todo!(),
                    }
                }
                code.rest_at = rest_at;
                code.param_count = params.len() as _;
                match &fun.body {
                    BlockStmtOrExpr::BlockStmt(block) => {
                        compiler.compile(&block.stmts);
                        compiler.emit(Opcode::OP_PUSH_UNDEF, &[], false);
                        compiler.emit(Opcode::OP_RET, &[], false);
                    }
                    BlockStmtOrExpr::Expr(expr) => {
                        compiler.expr(expr, true);
                        compiler.emit(Opcode::OP_RET, &[], false);
                    }
                }
                let code = compiler.finish(&mut self.rt);
                let ix = self.code.codes.len();
                self.code.codes.push(code);
                let _nix = self.get_sym(name);
                self.emit(Opcode::OP_GET_FUNCTION, &[ix as _], false);
            }
            Expr::Fn(fun) => {
                self.emit(Opcode::OP_PUSH_ENV, &[], false);
                self.push_scope();
                let name = fun
                    .ident
                    .as_ref()
                    .map(|x| Self::ident_to_sym(x))
                    .unwrap_or_else(|| "<anonymous>".intern());

                let mut params = vec![];
                let mut code = CodeBlock::new(&mut self.rt, name, false);

                let mut compiler = ByteCompiler {
                    lci: Vec::new(),
                    top_level: false,

                    code: code,
                    val_map: Default::default(),
                    name_map: Default::default(),

                    fmap: Default::default(),
                    rt: self.rt,
                    scope: Rc::new(RefCell::new(Scope {
                        parent: Some(self.scope.clone()),
                        depth: self.scope.borrow().depth + 1,
                        variables: HashMap::new(),
                    })),
                };
                let mut rest_at = None;
                for x in fun.function.params.iter() {
                    match x.pat {
                        Pat::Ident(ref x) => {
                            params.push(Self::ident_to_sym(&x.id));
                            compiler
                                .scope
                                .borrow_mut()
                                .add_var(Self::ident_to_sym(&x.id));
                        }
                        Pat::Rest(ref r) => match &*r.arg {
                            Pat::Ident(ref id) => {
                                rest_at = Some(
                                    compiler
                                        .scope
                                        .borrow_mut()
                                        .add_var(Self::ident_to_sym(&id.id))
                                        as u32,
                                );
                            }
                            _ => unreachable!(),
                        },
                        _ => todo!(),
                    }
                }
                code.param_count = params.len() as _;
                code.rest_at = rest_at;

                compiler.compile_fn(&fun.function);
                let code = compiler.finish(&mut self.rt);
                let ix = self.code.codes.len();
                self.code.codes.push(code);
                let _nix = self.get_sym(name);
                self.emit(Opcode::OP_GET_FUNCTION, &[ix as _], false);
                if name != "<anonymous>".intern() {
                    self.emit(Opcode::OP_DUP, &[], false);
                    let var = self.access_var(name);
                    self.access_set(var);
                    //   self.emit(Opcode::OP_SET_VAR, &[nix as _], true);
                }
                self.pop_scope();
                self.emit(Opcode::OP_POP_ENV, &[], false);
                if !used {
                    self.emit(Opcode::OP_POP, &[], false);
                }
            }

            Expr::Array(array_lit) => {
                for expr in array_lit.elems.iter().rev() {
                    match expr {
                        Some(expr) => {
                            self.expr(&expr.expr, true);
                            if expr.spread.is_some() {
                                self.emit(Opcode::OP_SPREAD, &[], false);
                            }
                        }
                        None => self.emit(Opcode::OP_PUSH_UNDEF, &[], false),
                    }
                }
                self.emit(Opcode::OP_NEWARRAY, &[array_lit.elems.len() as u32], false);
                if !used {
                    self.emit(Opcode::OP_POP, &[], false);
                }
            }

            Expr::Cond(cond) => {
                self.expr(&cond.test, true);
                let jelse = self.cjmp(false);
                self.expr(&cond.cons, used);

                let jend = self.jmp();
                jelse(self);
                self.expr(&cond.alt, used);
                jend(self);
            }
            Expr::Paren(p) => {
                self.expr(&p.expr, used);
            }
            x => todo!("{:?}", x),
        }
    }

    pub fn try_(&mut self) -> impl FnOnce(&mut Self) {
        let p = self.code.code.len();
        self.emit(Opcode::OP_PUSH_CATCH, &[0], false);

        move |this: &mut Self| {
            let to = this.code.code.len() - (p + 5);
            let ins = Opcode::OP_PUSH_CATCH;
            let bytes = (to as u32).to_le_bytes();
            this.code.code[p] = ins as u8;
            this.code.code[p + 1] = bytes[0];
            this.code.code[p + 2] = bytes[1];
            this.code.code[p + 3] = bytes[2];
            this.code.code[p + 4] = bytes[3];
        }
    }
    pub fn cjmp(&mut self, cond: bool) -> impl FnOnce(&mut Self) {
        let p = self.code.code.len();
        self.emit(Opcode::OP_JMP, &[0], false);

        move |this: &mut Self| {
            //  this.emit(Opcode::OP_NOP, &[], false);
            let to = this.code.code.len() - (p + 5);
            let ins = if cond {
                Opcode::OP_JMP_IF_TRUE
            } else {
                Opcode::OP_JMP_IF_FALSE
            };
            let bytes = (to as u32).to_le_bytes();
            this.code.code[p] = ins as u8;
            this.code.code[p + 1] = bytes[0];
            this.code.code[p + 2] = bytes[1];
            this.code.code[p + 3] = bytes[2];
            this.code.code[p + 4] = bytes[3];
        }
    }
    pub fn goto(&mut self, to: usize) {
        let at = self.code.code.len() as i32 + 5;
        self.emit(Opcode::OP_JMP, &[(to as i32 - at) as u32], false);
    }
    pub fn jmp(&mut self) -> impl FnOnce(&mut Self) {
        let p = self.code.code.len();
        self.emit(Opcode::OP_JMP, &[0], false);

        move |this: &mut Self| {
            // this.emit(Opcode::OP_NOP, &[], false);
            let to = this.code.code.len() - (p + 5);
            let bytes = (to as u32).to_le_bytes();
            this.code.code[p] = Opcode::OP_JMP as u8;
            this.code.code[p + 1] = bytes[0];
            this.code.code[p + 2] = bytes[1];
            this.code.code[p + 3] = bytes[2];
            this.code.code[p + 4] = bytes[3];
            //this.code.code[p] = ins as u8;
        }
    }

    pub fn jmp_custom(&mut self, op: Opcode) -> impl FnOnce(&mut Self) {
        let p = self.code.code.len();
        self.emit(op, &[0], false);

        move |this: &mut Self| {
            // this.emit(Opcode::OP_NOP, &[], false);
            let to = this.code.code.len() - (p + 5);
            let bytes = (to as u32).to_le_bytes();
            this.code.code[p] = op as u8;
            this.code.code[p + 1] = bytes[0];
            this.code.code[p + 2] = bytes[1];
            this.code.code[p + 3] = bytes[2];
            this.code.code[p + 4] = bytes[3];
            //this.code.code[p] = ins as u8;
        }
    }
    // fn declare_variable(&mut self,decl: &VarDecl) -> Vec<u32>
    pub fn emit(&mut self, op: Opcode, operands: &[u32], add_feedback: bool) {
        self.code.code.push(op as u8);
        for operand in operands.iter() {
            for x in operand.to_le_bytes().iter() {
                self.code.code.push(*x);
            }
        }
        if add_feedback {
            let f_ix = self.code.feedback.len() as u32;
            self.code.feedback.push(TypeFeedBack::None);
            for x in f_ix.to_le_bytes().iter() {
                self.code.code.push(*x);
            }
        }
    }

    pub fn emit_u8(&mut self, x: u8) {
        self.code.code.push(x);
    }

    pub fn emit_u16(&mut self, x: u16) {
        let bytes = x.to_le_bytes();
        self.code.code.extend(&bytes);
    }

    pub fn emit_u32(&mut self, x: u32) {
        self.code.code.extend(&x.to_le_bytes());
    }
}

impl<'a> VisitFnDecl<'a> {
    pub fn visit(stmts: &[Stmt], clos: &'a mut dyn FnMut(&FnDecl)) {
        let mut visit = Self { cb: clos };
        for stmt in stmts.iter() {
            stmt.visit_with(&Invalid { span: DUMMY_SP }, &mut visit)
        }
    }
}

pub struct VisitFnDecl<'a> {
    cb: &'a mut dyn FnMut(&FnDecl),
}

impl<'a> Visit for VisitFnDecl<'a> {
    fn visit_fn_decl(&mut self, n: &FnDecl, _: &dyn Node) {
        (self.cb)(n);
    }
}
pub trait IsDirective {
    fn as_ref(&self) -> Option<&Stmt>;
    fn is_use_strict(&self) -> bool {
        match self.as_ref() {
            Some(&Stmt::Expr(ref expr)) => match *expr.expr {
                Expr::Lit(Lit::Str(Str {
                    ref value,
                    has_escape: false,
                    ..
                })) => value == "use strict",
                _ => false,
            },
            _ => false,
        }
    }
}

impl IsDirective for Stmt {
    fn as_ref(&self) -> Option<&Stmt> {
        Some(self)
    }
}

pub fn contains_ident<'a, N>(body: &N, ident: &'a str) -> bool
where
    N: VisitWith<IdentFinder<'a>>,
{
    let mut visitor = IdentFinder {
        found: false,
        ident,
    };
    body.visit_with(&Invalid { span: DUMMY_SP } as _, &mut visitor);
    visitor.found
}
pub struct IdentFinder<'a> {
    ident: &'a str,
    found: bool,
}

impl Visit for IdentFinder<'_> {
    noop_visit_type!();

    fn visit_expr(&mut self, e: &Expr, _: &dyn Node) {
        e.visit_children_with(self);

        match *e {
            Expr::Ident(ref i) if &i.sym == self.ident => {
                self.found = true;
            }
            _ => {}
        }
    }
}