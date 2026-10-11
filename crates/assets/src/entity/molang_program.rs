use super::{
    CompiledMolangExpression, MolangCollection, MolangCollectionItem, MolangOp, MolangQuery,
    MolangSymbol, bind_molang_queries,
};

/// Owned bytecode and its symbol table for expressions admitted independently of a carrier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MolangProgram {
    query_bindings: Box<[Option<MolangQuery>]>,
    pub symbols: Box<[MolangSymbol]>,
    pub expressions: Box<[CompiledMolangExpression]>,
    pub ops: Box<[MolangOp]>,
    pub collections: Box<[MolangCollection]>,
    pub collection_items: Box<[MolangCollectionItem]>,
}

impl MolangProgram {
    /// Builds a program and binds every query symbol before evaluation.
    pub fn new(
        symbols: Box<[MolangSymbol]>,
        expressions: Box<[CompiledMolangExpression]>,
        ops: Box<[MolangOp]>,
        collections: Box<[MolangCollection]>,
        collection_items: Box<[MolangCollectionItem]>,
    ) -> Self {
        Self {
            query_bindings: bind_molang_queries(&symbols),
            symbols,
            expressions,
            ops,
            collections,
            collection_items,
        }
    }

    /// Reads the handler already bound to this symbol slot.
    pub fn query_binding(&self, symbol: u32) -> Option<MolangQuery> {
        self.query_bindings.get(symbol as usize).copied().flatten()
    }
}

impl super::RuntimeEntityAssets {
    /// Reads a query binding populated when this carrier was admitted.
    pub fn molang_query_binding(&self, symbol: u32) -> Option<MolangQuery> {
        self.molang_query_bindings
            .get(symbol as usize)
            .copied()
            .flatten()
    }
}
