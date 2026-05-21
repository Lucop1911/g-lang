use crate::ast::ast::{Expr, Program, SlotIndex, Stmt};

pub(crate) fn compute_slots(program: &mut Program) {
    let mut scope = Scope::new();
    scope.process_program(program);
}

struct Scope {
    // depth: usize, // For debugging
}

impl Scope {
    fn new() -> Self {
        // Scope { depth: 0 } For debugging
        Scope {}
    }

    fn process_program(&mut self, program: &mut Program) {
        /*
        Top-level variables use global name-based lookup (UNSET slots).
        This is consistent with the VM's GetGlobal/SetGlobal which use
        name-based lookup in the globals Environment.

        We still recurse into function bodies so that param/local idents
        inside those bodies get correct 0-based slot indices.
        */

        let running_locals: Vec<(String, SlotIndex)> = Vec::new();

        for stmt in program.iter_mut() {
            match stmt {
                Stmt::LetStmt(ident, expr) => {
                    self.process_expr(expr, &running_locals);
                    // Top-level let: use UNSET for name-based global lookup.
                    ident.slot = SlotIndex::UNSET;
                }
                Stmt::FnStmt {
                    params: fn_params,
                    body,
                    ..
                } => {
                    // Params always start at slot 0 inside their own frame.
                    let fn_params_locals: Vec<(String, SlotIndex)> = fn_params
                        .iter_mut()
                        .enumerate()
                        .map(|(i, p)| {
                            p.slot = SlotIndex(i as u16);
                            (p.name.clone(), p.slot)
                        })
                        .collect();
                    // running_locals has UNSET slots for top-level lets, which is fine:
                    // process_expr will mark those idents UNSET too so the evaluator uses name lookup for them.
                    self.process_fn_body(body, &running_locals, &fn_params_locals);
                }
                Stmt::AssignStmt(ident, expr) => {
                    self.process_expr(expr, &running_locals);
                    // Top-level assign target stays UNSET (name lookup).
                    ident.slot = SlotIndex::UNSET;
                }
                Stmt::ExprStmt(e)
                | Stmt::ExprValueStmt(e)
                | Stmt::ReturnStmt(e)
                | Stmt::ThrowStmt(e) => {
                    self.process_expr(e, &running_locals);
                }
                Stmt::FieldAssignStmt { object, value, .. } => {
                    self.process_expr(object, &running_locals);
                    self.process_expr(value, &running_locals);
                }
                Stmt::IndexAssignStmt {
                    target,
                    index,
                    value,
                } => {
                    self.process_expr(target, &running_locals);
                    self.process_expr(index, &running_locals);
                    self.process_expr(value, &running_locals);
                }
                Stmt::StructStmt { fields, methods, .. } => {
                    for (_, expr) in fields {
                        self.process_expr(expr, &running_locals);
                    }
                    for (_, expr) in methods {
                        self.process_expr(expr, &running_locals);
                    }
                }
                _ => {}
            }
        }
    }

    /* Process a function body.

    - `parent_locals`: names visible from enclosing scopes. Entries with
         UNSET slots cause idents to stay UNSET (name-based lookup).
    - `fn_params`: this function's params, already assigned 0-based slots.

    Local `let` slots start at `fn_params.len()` so they never collide with
    params. This matches `Environment::count_slots(params, body)` which
    allocates `params.len() + let_count` slots for the frame.
    */
    fn process_fn_body(
        &mut self,
        body: &mut Program,
        parent_locals: &[(String, SlotIndex)],
        fn_params: &[(String, SlotIndex)],
    ) {
        // self.depth += 1; // For debugging

        let mut local_slot_idx = fn_params.len();

        let mut let_slots: Vec<(String, SlotIndex)> = Vec::new();
        for stmt in body.iter() {
            match stmt {
                Stmt::LetStmt(ident, _) => {
                    let_slots.push((ident.name.clone(), SlotIndex(local_slot_idx as u16)));
                    local_slot_idx += 1;
                }
                Stmt::MultiLetStmt { idents, values: _ } => {
                    for ident in idents {
                        let_slots.push((ident.name.clone(), SlotIndex(local_slot_idx as u16)));
                        local_slot_idx += 1;
                    }
                }
                _ => {}
            }
        }

        // Write let slots back into the ident nodes.
        let mut let_iter = let_slots.iter();
        for stmt in body.iter_mut() {
            match stmt {
                Stmt::LetStmt(ident, _) => {
                    if let Some((_, slot)) = let_iter.next() {
                        ident.slot = *slot;
                    }
                }
                Stmt::MultiLetStmt { idents, values: _ } => {
                    for ident in idents {
                        if let Some((_, slot)) = let_iter.next() {
                            ident.slot = *slot;
                        }
                    }
                }
                _ => {}
            }
        }

        /*
        Compose visible names. Later entries shadow earlier ones;
        process_expr searches with .rev().
        Parent locals are captured via the closure env chain at runtime, so
        they must be looked up by NAME (slot = UNSET). If we kept their slot
        indices here, a parent's slot 0 would collide with this function's
        slot 0 and the wrong value would be read from the current frame.
        */
        let mut expr_locals: Vec<(String, SlotIndex)> = parent_locals
            .iter()
            .map(|(n, _)| (n.clone(), SlotIndex::UNSET))
            .collect();
        for (n, s) in fn_params.iter() {
            expr_locals.push((n.clone(), *s));
        }
        for (n, s) in let_slots.iter() {
            expr_locals.push((n.clone(), *s));
        }

        for stmt in body.iter_mut() {
            match stmt {
                Stmt::FnStmt {
                    params: nested_params,
                    body: fn_body,
                    ..
                } => {
                    let nested_fn_params: Vec<(String, SlotIndex)> = nested_params
                        .iter_mut()
                        .enumerate()
                        .map(|(i, p)| {
                            p.slot = SlotIndex(i as u16);
                            (p.name.clone(), p.slot)
                        })
                        .collect();
                    self.process_fn_body(fn_body, &expr_locals, &nested_fn_params);
                }
                Stmt::LetStmt(_, expr) => {
                    self.process_expr(expr, &expr_locals);
                }
                Stmt::AssignStmt(ident, expr) => {
                    self.process_expr(expr, &expr_locals);
                    if let Some((_, slot)) =
                        expr_locals.iter().rev().find(|(n, _)| n == &ident.name)
                    {
                        ident.slot = *slot;
                    }
                }
                Stmt::ExprStmt(e)
                | Stmt::ExprValueStmt(e)
                | Stmt::ReturnStmt(e)
                | Stmt::ThrowStmt(e) => {
                    self.process_expr(e, &expr_locals);
                }
                Stmt::FieldAssignStmt { object, value, .. } => {
                    self.process_expr(object, &expr_locals);
                    self.process_expr(value, &expr_locals);
                }
                Stmt::IndexAssignStmt {
                    target,
                    index,
                    value,
                } => {
                    self.process_expr(target, &expr_locals);
                    self.process_expr(index, &expr_locals);
                    self.process_expr(value, &expr_locals);
                }
                Stmt::MultiLetStmt { idents: _, values } => {
                    for val in values {
                        self.process_expr(val, &expr_locals);
                    }
                }
                Stmt::TupleAssignStmt { targets, values } => {
                    for val in values {
                        self.process_expr(val, &expr_locals);
                    }
                    for trgt in targets {
                        if let Some((_, slot)) =
                            expr_locals.iter().rev().find(|(n, _)| n == &trgt.name)
                        {
                            trgt.slot = *slot;
                        }
                    }
                }
                _ => {}
            }
        }

        // self.depth -= 1; // For debugging
    }

    /// Process a block body that shares the same frame as its parent
    /// (while, for, if, try/catch bodies).
    ///
    /// Unlike `process_fn_body`, this preserves parent slot indices
    /// instead of converting them to UNSET, because these blocks run
    /// in the same frame and need O(1) slot access.
    fn process_block_body(&mut self, body: &mut Program, parent_locals: &[(String, SlotIndex)]) {
        let mut local_slot_idx = parent_locals.len();

        let mut let_slots: Vec<(String, SlotIndex)> = Vec::new();
        for stmt in body.iter() {
            match stmt {
                Stmt::LetStmt(ident, _) => {
                    let_slots.push((ident.name.clone(), SlotIndex(local_slot_idx as u16)));
                    local_slot_idx += 1;
                }
                Stmt::MultiLetStmt { idents, values: _ } => {
                    for ident in idents {
                        let_slots.push((ident.name.clone(), SlotIndex(local_slot_idx as u16)));
                        local_slot_idx += 1;
                    }
                }
                _ => {}
            }
        }

        // Write let slots back into the ident nodes.
        let mut let_iter = let_slots.iter();
        for stmt in body.iter_mut() {
            match stmt {
                Stmt::LetStmt(ident, _) => {
                    if let Some((_, slot)) = let_iter.next() {
                        ident.slot = *slot;
                    }
                }
                Stmt::MultiLetStmt { idents, values: _ } => {
                    for ident in idents {
                        if let Some((_, slot)) = let_iter.next() {
                            ident.slot = *slot;
                        }
                    }
                }
                _ => {}
            }
        }

        // Compose visible names — KEEP parent slot indices (not UNSET)
        let mut expr_locals: Vec<(String, SlotIndex)> = parent_locals.to_vec();
        for (n, s) in let_slots.iter() {
            expr_locals.push((n.clone(), *s));
        }

        for stmt in body.iter_mut() {
            match stmt {
                Stmt::FnStmt {
                    params: nested_params,
                    body: fn_body,
                    ..
                } => {
                    let nested_fn_params: Vec<(String, SlotIndex)> = nested_params
                        .iter_mut()
                        .enumerate()
                        .map(|(i, p)| {
                            p.slot = SlotIndex(i as u16);
                            (p.name.clone(), p.slot)
                        })
                        .collect();
                    self.process_fn_body(fn_body, &expr_locals, &nested_fn_params);
                }
                Stmt::LetStmt(_, expr) => {
                    self.process_expr(expr, &expr_locals);
                }
                Stmt::AssignStmt(ident, expr) => {
                    self.process_expr(expr, &expr_locals);
                    if let Some((_, slot)) =
                        expr_locals.iter().rev().find(|(n, _)| n == &ident.name)
                    {
                        ident.slot = *slot;
                    }
                }
                Stmt::ExprStmt(e)
                | Stmt::ExprValueStmt(e)
                | Stmt::ReturnStmt(e)
                | Stmt::ThrowStmt(e) => {
                    self.process_expr(e, &expr_locals);
                }
                Stmt::FieldAssignStmt { object, value, .. } => {
                    self.process_expr(object, &expr_locals);
                    self.process_expr(value, &expr_locals);
                }
                Stmt::IndexAssignStmt {
                    target,
                    index,
                    value,
                } => {
                    self.process_expr(target, &expr_locals);
                    self.process_expr(index, &expr_locals);
                    self.process_expr(value, &expr_locals);
                }
                Stmt::MultiLetStmt { idents: _, values } => {
                    for val in values {
                        self.process_expr(val, &expr_locals);
                    }
                }
                Stmt::TupleAssignStmt { targets, values } => {
                    for val in values {
                        self.process_expr(val, &expr_locals);
                    }
                    for trgt in targets {
                        if let Some((_, slot)) =
                            expr_locals.iter().rev().find(|(n, _)| n == &trgt.name)
                        {
                            trgt.slot = *slot;
                        }
                    }
                }
                Stmt::StructStmt { fields, methods, .. } => {
                    for (_, expr) in fields {
                        self.process_expr(expr, &expr_locals);
                    }
                    for (_, expr) in methods {
                        self.process_expr(expr, &expr_locals);
                    }
                }
                _ => {}
            }
        }
    }

    fn process_expr(&mut self, expr: &mut Expr, locals: &[(String, SlotIndex)]) {
        match expr {
            Expr::IdentExpr(ident) => {
                if let Some((_, slot)) = locals.iter().rev().find(|(n, _)| n == &ident.name) {
                    ident.slot = *slot;
                } else {
                    ident.slot = SlotIndex::UNSET;
                }
            }
            Expr::FnExpr { params, body } => {
                let fn_params: Vec<(String, SlotIndex)> = params
                    .iter_mut()
                    .enumerate()
                    .map(|(i, p)| {
                        p.slot = SlotIndex(i as u16);
                        (p.name.clone(), p.slot)
                    })
                    .collect();
                self.process_fn_body(body, locals, &fn_params);
            }

            Expr::AsyncFnExpr { params, body } => {
                let fn_params: Vec<(String, SlotIndex)> = params
                    .iter_mut()
                    .enumerate()
                    .map(|(i, p)| {
                        p.slot = SlotIndex(i as u16);
                        (p.name.clone(), p.slot)
                    })
                    .collect();
                self.process_fn_body(body, locals, &fn_params);
            }

            Expr::IfExpr {
                cond,
                consequence,
                alternative,
            } => {
                self.process_expr(cond, locals);
                self.process_block_body(consequence, locals);
                if let Some(alt) = alternative {
                    self.process_block_body(alt, locals);
                }
            }

            Expr::WhileExpr { cond, body } => {
                self.process_expr(cond, locals);
                self.process_block_body(body, locals);
            }

            Expr::ForExpr {
                ident,
                iterable,
                body,
            } => {
                self.process_expr(iterable, locals);
                let mut for_locals = locals.to_vec();
                for id in ident {
                    let loop_slot = SlotIndex(for_locals.len() as u16);
                    id.slot = loop_slot;
                    for_locals.push((id.name.clone(), loop_slot));
                }
                self.process_block_body(body, &for_locals);
            }

            Expr::CStyleForExpr {
                init,
                cond,
                update,
                body,
            } => {
                let mut for_locals = locals.to_vec();
                if let Some(init_stmt) = init {
                    self.process_stmt_extending(init_stmt, &mut for_locals);
                }
                if let Some(c) = cond {
                    self.process_expr(c, &for_locals);
                }
                self.process_block_body(body, &for_locals);
                if let Some(u) = update {
                    self.process_stmt_extending(u, &mut for_locals);
                }
            }

            Expr::TryCatchExpr {
                try_body,
                catch_ident,
                catch_body,
                finally_body,
            } => {
                self.process_block_body(try_body, locals);
                if let Some(cb) = catch_body {
                    let mut catch_locals = locals.to_vec();
                    if let Some(ident) = catch_ident {
                        let catch_slot = SlotIndex(locals.len() as u16);
                        ident.slot = catch_slot;
                        catch_locals.push((ident.name.clone(), catch_slot));
                    }
                    self.process_block_body(cb, &catch_locals);
                }
                if let Some(fb) = finally_body {
                    self.process_block_body(fb, locals);
                }
            }

            Expr::PrefixExpr(_, e) => self.process_expr(e, locals),
            Expr::InfixExpr(_, l, r) => {
                self.process_expr(l, locals);
                self.process_expr(r, locals);
            }
            Expr::CallExpr {
                function,
                arguments,
            } => {
                self.process_expr(function, locals);
                for a in arguments {
                    self.process_expr(a, locals);
                }
            }
            Expr::ArrayExpr(es) => {
                for e in es {
                    self.process_expr(e, locals);
                }
            }
            Expr::HashExpr(kvs) => {
                for (k, v) in kvs {
                    self.process_expr(k, locals);
                    self.process_expr(v, locals);
                }
            }
            Expr::IndexExpr { array, index } => {
                self.process_expr(array, locals);
                self.process_expr(index, locals);
            }
            Expr::MethodCallExpr {
                object, arguments, ..
            } => {
                self.process_expr(object, locals);
                for a in arguments {
                    self.process_expr(a, locals);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, e) in fields {
                    self.process_expr(e, locals);
                }
            }
            Expr::FieldAccessExpr { object, .. } => {
                self.process_expr(object, locals);
            }
            Expr::AwaitExpr(e) => self.process_expr(e, locals),
            Expr::MatchExpr { value, arms } => {
                self.process_expr(value, locals);
                for (_, body) in arms {
                    self.process_block_body(body, locals);
                }
            }
            Expr::LitExpr(_) | Expr::ThisExpr => {}
        }
    }

    fn process_stmt_extending(&mut self, stmt: &mut Stmt, locals: &mut Vec<(String, SlotIndex)>) {
        match stmt {
            Stmt::LetStmt(ident, expr) => {
                self.process_expr(expr, locals);
                ident.slot = SlotIndex(locals.len() as u16);
                locals.push((ident.name.clone(), ident.slot));
            }
            Stmt::AssignStmt(ident, expr) => {
                self.process_expr(expr, locals);
                if let Some((_, slot)) = locals.iter().rev().find(|(n, _)| n == &ident.name) {
                    ident.slot = *slot;
                }
            }
            Stmt::ExprStmt(e) | Stmt::ExprValueStmt(e) => {
                self.process_expr(e, locals);
            }
            _ => {}
        }
    }
}
