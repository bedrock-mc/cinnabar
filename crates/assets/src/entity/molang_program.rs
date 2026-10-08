use super::{
    CompiledMolangExpression, MolangCollection, MolangCollectionItem, MolangOp, MolangSymbol,
};

/// Owned bytecode and its symbol table for expressions admitted independently of a carrier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MolangProgram {
    pub symbols: Box<[MolangSymbol]>,
    pub expressions: Box<[CompiledMolangExpression]>,
    pub ops: Box<[MolangOp]>,
    pub collections: Box<[MolangCollection]>,
    pub collection_items: Box<[MolangCollectionItem]>,
}
