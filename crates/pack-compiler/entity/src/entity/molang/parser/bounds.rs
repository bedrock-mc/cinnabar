//! Checks owned-tree depth while each new expression still has bounded children.

use super::{AssetError, Expr, Statement, check_depth};

enum Node<'a> {
    Expression(&'a Expr),
    Statement(&'a Statement),
}

/// Rejects a new expression before further parents can deepen its recursively owned tree.
pub(super) fn bounded(expression: Expr) -> Result<Expr, AssetError> {
    let mut pending = vec![(Node::Expression(&expression), 0)];
    while let Some((node, depth)) = pending.pop() {
        check_depth(depth)?;
        let child = depth + 1;
        match node {
            Node::Expression(expression) => match expression {
                Expr::Number(_) | Expr::String(_) | Expr::This | Expr::Variable(..) => {}
                Expr::Query(_, None) => {}
                Expr::Query(_, Some(arguments)) | Expr::Call(_, arguments) => {
                    pending.extend(
                        arguments
                            .iter()
                            .map(|value| (Node::Expression(value), child)),
                    );
                }
                Expr::Unary(_, value) | Expr::Coalesce(_, _, value) => {
                    pending.push((Node::Expression(value), child));
                }
                Expr::Binary(_, left, right)
                | Expr::And(left, right)
                | Expr::Or(left, right)
                | Expr::Arrow(left, right) => {
                    pending.push((Node::Expression(left), child));
                    pending.push((Node::Expression(right), child));
                }
                Expr::Conditional(condition, yes, no) => {
                    pending.push((Node::Expression(condition), child));
                    pending.push((Node::Expression(yes), child));
                    if let Some(no) = no {
                        pending.push((Node::Expression(no), child));
                    }
                }
                Expr::Branch(condition, yes, no) => {
                    pending.push((Node::Expression(condition), child));
                    pending.extend(yes.iter().map(|value| (Node::Statement(value), child)));
                    if let Some(no) = no {
                        pending.extend(no.iter().map(|value| (Node::Statement(value), child)));
                    }
                }
            },
            Node::Statement(statement) => match statement {
                Statement::Expression(value) | Statement::Assign(_, _, value) => {
                    pending.push((Node::Expression(value), child));
                }
                Statement::Return(value) => {
                    if let Some(value) = value {
                        pending.push((Node::Expression(value), child));
                    }
                }
                Statement::Loop(value, body) | Statement::ForEach(_, _, value, body) => {
                    pending.push((Node::Expression(value), child));
                    pending.extend(body.iter().map(|value| (Node::Statement(value), child)));
                }
                Statement::Block(body) => {
                    pending.extend(body.iter().map(|value| (Node::Statement(value), child)));
                }
                Statement::Break | Statement::Continue => {}
            },
        }
    }
    Ok(expression)
}
