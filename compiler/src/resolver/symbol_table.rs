use crate::types::Type;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub ty: Type,
    pub is_mutable: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    scopes: Vec<HashMap<String, Symbol>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        let mut st = Self { scopes: Vec::new() };
        st.push_scope();
        st
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn insert(&mut self, symbol: Symbol) -> bool {
        if let Some(current_scope) = self.scopes.last_mut() {
            if current_scope.contains_key(&symbol.name) {
                false
            } else {
                current_scope.insert(symbol.name.clone(), symbol);
                true
            }
        } else {
            false
        }
    }

    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(sym) = scope.get(name) {
                return Some(sym);
            }
        }
        None
    }
}
