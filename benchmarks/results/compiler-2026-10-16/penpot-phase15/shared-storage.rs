impl ArenaStorage {
    fn shared<T>(
        &mut self,
        name: &'static str,
        value: &crate::shared::Shared<T>,
        storage: impl FnOnce(&T) -> (usize, usize),
    ) {
        if !self.seen.insert(&**value as *const T as usize) {
            return;
        }
        let (entries, bytes) = storage(value);
        let counts = self.tables.entry(name).or_default();
        counts.0 += entries;
        counts.1 += bytes + std::mem::size_of::<T>() + 2 * std::mem::size_of::<usize>();
        counts.2 += 1;
    }
    pub fn shared_program(&mut self, p: &crate::Program) {
        use std::mem::size_of;
        self.shared("shared/namespaces", &p.namespaces, |v| {
            (v.len(), v.capacity() * size_of::<String>() + v.iter().map(String::capacity).sum::<usize>())
        });
        self.shared("shared/namespace_entries", &p.namespace_entries, |v| {
            (v.len(), v.capacity() * size_of::<(crate::ExprId, String)>() + v.values().map(String::capacity).sum::<usize>())
        });
        self.shared("shared/protocols", &p.protocols, |v| {
            (v.len(), v.capacity() * size_of::<(crate::analyzer::GlobalId, Vec<crate::analyzer::ProtocolMethod>)>()
                + v.values().map(|methods| methods.capacity() * size_of::<crate::analyzer::ProtocolMethod>()
                    + methods.iter().map(|method| method.key.capacity() + method.arities.capacity() * size_of::<usize>()).sum::<usize>()).sum::<usize>())
        });
        self.shared("shared/record_constructors", &p.record_constructors, |v| {
            (v.len(), v.capacity() * size_of::<crate::analyzer::GlobalId>())
        });
        self.shared("shared/field_bindings", &p.field_bindings, |v| {
            (v.len(), v.capacity() * size_of::<(crate::analyzer::LocalId, (crate::analyzer::RecurTargetId, bool))>())
        });
        self.shared("shared/type_methods", &p.type_methods, |v| {
            (v.len(), v.capacity() * size_of::<crate::analyzer::RecurTargetId>())
        });
        self.shared("shared/macro_functions", &p.macro_functions, |v| {
            (v.len(), v.capacity() * size_of::<crate::ExprId>())
        });
        self.shared("shared/namespace_requires", &p.namespace_requires, |v| {
            (v.len(), v.capacity() * size_of::<Vec<usize>>() + v.iter().map(|v| v.capacity() * size_of::<usize>()).sum::<usize>())
        });
        self.shared("shared/namespace_lazy_only", &p.namespace_lazy_only, |v| (v.len(), v.capacity()));
        self.shared("shared/lazy_targets", &p.lazy_targets, |v| {
            (v.len(), v.capacity() * size_of::<crate::analyzer::LazyTarget>())
        });
        self.shared("shared/lazy_namespaces", &p.lazy_namespaces, |v| {
            (v.len(), v.capacity() * size_of::<String>() + v.iter().map(String::capacity).sum::<usize>())
        });
    }
}
