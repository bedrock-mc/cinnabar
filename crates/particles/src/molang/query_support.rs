//! Named actor query requirements and explicitly unsupported script calls.
use super::*;

impl Program {
    /// Lists external numeric queries and rejects calls outside the implemented expression grammar.
    pub(crate) fn actor_queries(&self) -> Option<Box<[Box<str>]>> {
        let mut names = std::collections::BTreeSet::new();
        for statement in &self.stmts {
            collect(statement, &mut names)?;
        }
        Some(names.into_iter().collect())
    }
}

/// Walks the bounded parsed tree once at pack admission, retaining query names without reparsing frames.
fn collect(expression: &Expr, names: &mut std::collections::BTreeSet<Box<str>>) -> Option<()> {
    match expression {
        Expr::Query(Query::Named(name)) => {
            names.insert(name.clone());
        }
        Expr::Neg(value)
        | Expr::Not(value)
        | Expr::AssignVar(_, value)
        | Expr::AssignTemp(_, value)
        | Expr::Return(value) => collect(value, names)?,
        Expr::Bin(_, left, right) | Expr::Coalesce(left, right) => {
            collect(left, names)?;
            collect(right, names)?;
        }
        Expr::Cond(condition, yes, no) => {
            collect(condition, names)?;
            collect(yes, names)?;
            collect(no, names)?;
        }
        Expr::Call(None, _) => return None,
        Expr::Call(_, arguments) => {
            for value in arguments {
                collect(value, names)?;
            }
        }
        _ => {}
    }
    Some(())
}
