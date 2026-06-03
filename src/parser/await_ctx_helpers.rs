use crate::{
    ast::ast::{Expr, Program, Stmt},
    vm::runtime::runtime_errors::ParserError,
};

fn verify_await_in_async(program: &Program, in_async: bool) -> Result<(), ParserError> {
    for stmt in program {
        verify_await_in_stmt(stmt, in_async)?;
    }
    Ok(())
}

fn verify_await_in_stmt(stmt: &Stmt, in_async: bool) -> Result<(), ParserError> {
    match stmt {
        Stmt::LetStmt(_, expr)
        | Stmt::AssignStmt(_, expr)
        | Stmt::ExprStmt(expr)
        | Stmt::ExprValueStmt(expr)
        | Stmt::ReturnStmt(expr)
        | Stmt::ThrowStmt(expr) => verify_await_in_expr(expr, in_async),
        Stmt::FnStmt { body, .. } => {
            for s in body {
                verify_await_in_stmt(s, false)?;
            }
            Ok(())
        }
        Stmt::TupleAssignStmt { targets: _, values } => {
            for expr in values {
                verify_await_in_expr(expr, in_async)?;
            }
            Ok(())
        }
        Stmt::MultiLetStmt { idents: _, values } => {
            for expr in values {
                verify_await_in_expr(expr, in_async)?;
            }
            Ok(())
        }
        Stmt::BreakStmt | Stmt::ContinueStmt => Ok(()),
        Stmt::ImportStmt { .. } | Stmt::FieldAssignStmt { .. } | Stmt::IndexAssignStmt { .. } => {
            Ok(())
        }
        Stmt::StructStmt { methods, .. } => {
            for (_, expr) in methods {
                verify_await_in_expr(expr, false)?;
            }
            Ok(())
        }
        Stmt::EnumStmt { .. } => Ok(()),
    }
}

fn verify_await_in_expr(expr: &Expr, in_async: bool) -> Result<(), ParserError> {
    match expr {
        Expr::IdentExpr(_) | Expr::LitExpr(_) => Ok(()),
        Expr::PrefixExpr(_, e) => verify_await_in_expr(e, in_async),
        Expr::InfixExpr(_, e1, e2) => {
            verify_await_in_expr(e1, in_async)?;
            verify_await_in_expr(e2, in_async)
        }
        Expr::IfExpr {
            cond,
            consequence,
            alternative,
        } => {
            verify_await_in_expr(cond, in_async)?;
            for s in consequence {
                verify_await_in_stmt(s, in_async)?;
            }
            if let Some(alt) = alternative {
                for s in alt {
                    verify_await_in_stmt(s, in_async)?;
                }
            }
            Ok(())
        }
        Expr::FnExpr { body, .. } => {
            for s in body {
                verify_await_in_stmt(s, false)?;
            }
            Ok(())
        }
        Expr::AsyncFnExpr { body, .. } => {
            for s in body {
                verify_await_in_stmt(s, true)?;
            }
            Ok(())
        }
        Expr::CallExpr {
            function,
            arguments,
        } => {
            verify_await_in_expr(function, in_async)?;
            for arg in arguments {
                verify_await_in_expr(arg, in_async)?;
            }
            Ok(())
        }
        Expr::ArrayExpr(arr) => {
            for e in arr {
                verify_await_in_expr(e, in_async)?;
            }
            Ok(())
        }
        Expr::HashExpr(pairs) => {
            for (k, v) in pairs {
                verify_await_in_expr(k, in_async)?;
                verify_await_in_expr(v, in_async)?;
            }
            Ok(())
        }
        Expr::IndexExpr { array, index } => {
            verify_await_in_expr(array, in_async)?;
            verify_await_in_expr(index, in_async)
        }
        Expr::MethodCallExpr {
            object,
            method: _,
            arguments,
        } => {
            verify_await_in_expr(object, in_async)?;
            for arg in arguments {
                verify_await_in_expr(arg, in_async)?;
            }
            Ok(())
        }
        Expr::ForExpr { .. } => Ok(()),
        Expr::WhileExpr { .. } => Ok(()),
        Expr::TryCatchExpr {
            try_body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in try_body {
                verify_await_in_stmt(s, in_async)?;
            }
            if let Some(cb) = catch_body {
                for s in cb {
                    verify_await_in_stmt(s, in_async)?;
                }
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    verify_await_in_stmt(s, in_async)?;
                }
            }
            Ok(())
        }
        Expr::AwaitExpr(_) => {
            if in_async {
                Ok(())
            } else {
                Err(ParserError::AwaitOutsideAsync { location: None })
            }
        }
        Expr::MatchExpr { value, arms } => {
            verify_await_in_expr(value, in_async)?;
            for (_, body) in arms {
                for s in body {
                    verify_await_in_stmt(s, in_async)?;
                }
            }
            Ok(())
        }
        Expr::StructLiteral { .. }
        | Expr::EnumStructLiteral { .. }
        | Expr::ThisExpr
        | Expr::FieldAccessExpr { .. }
        | Expr::CStyleForExpr { .. } => Ok(()),
    }
}

pub fn validate_await_usage(program: &Program) -> Result<(), ParserError> {
    verify_await_in_async(program, false)
}
