//! Module grammar and static semantics (ECMA-262 ParseModule). The ordinary
//! parser still handles every executable declaration and expression.
//! QuickJS's js_parse_import/js_parse_export were consulted, not adapted.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::frontend::ast::{DefaultExport, Module, ModuleItem};
use crate::frontend::{
    ImportEntry, ImportName, IndirectExportEntry, LocalExportEntry, ModuleRecord,
};
use crate::value::JsString;

#[derive(Default)]
struct Tables {
    requested: Vec<JsString>,
    requested_set: BTreeSet<JsString>,
    imports: Vec<ImportEntry>,
    exports: Vec<(Ident, Ident)>, // (local name, exported name)
    exported_names: BTreeSet<String>,
    indirect: Vec<IndirectExportEntry>,
    stars: Vec<JsString>,
}

impl Tables {
    fn request(&mut self, specifier: &JsString) {
        if self.requested_set.insert(specifier.clone()) {
            self.requested.push(specifier.clone());
        }
    }

    fn export(&mut self, p: &Parser<'_>, local: Ident, exported: Ident) -> PResult<()> {
        if !self.exported_names.insert(exported.name.clone()) {
            return Err(p.error(
                SyntaxErrorKind::Invalid,
                exported.span,
                format!("duplicate exported name `{}`", exported.name),
            ));
        }
        self.exports.push((local, exported));
        Ok(())
    }

    fn finish(self, p: &Parser<'_>, body: Vec<ModuleItem>) -> PResult<Module> {
        // Module functions, unlike script functions, are lexical declarations.
        // VarDeclaredNames includes nested statements, but not nested functions.
        let mut declarations: Vec<(&str, Span, bool)> = self
            .imports
            .iter()
            .map(|i| (i.local_name.as_str(), i.span, true))
            .collect();
        for item in &body {
            match item {
                ModuleItem::Statement(stmt) => {
                    match &stmt.kind {
                        StmtKind::FunctionDecl(f) => {
                            if let Some(name) = &f.name {
                                declarations.push((&name.name, name.span, true));
                            }
                        }
                        StmtKind::Lexical(decl) => {
                            for name in decl.declarators.iter().flat_map(|d| d.name.names()) {
                                declarations.push((&name.name, name.span, true));
                            }
                        }
                        _ => {}
                    }
                    let mut vars = Vec::new();
                    var_declared_names(stmt, &mut vars);
                    declarations.extend(vars.into_iter().map(|i| (i.name.as_str(), i.span, false)));
                }
                ModuleItem::DefaultExport(DefaultExport::Function(f)) => {
                    if let Some(name) = &f.name {
                        declarations.push((&name.name, name.span, true));
                    }
                }
                ModuleItem::DefaultExport(DefaultExport::Expression(_)) => {}
                ModuleItem::DefaultExport(DefaultExport::Class(class)) => {
                    if let Some(name) = &class.name {
                        declarations.push((&name.name, name.span, true));
                    }
                }
            }
        }
        declarations.sort_by_key(|(_, span, _)| span.start);
        let mut bound = BTreeMap::new();
        for (name, span, lexical) in declarations {
            if let Some(prior_lexical) = bound.insert(name, lexical)
                && (lexical || prior_lexical)
            {
                return Err(p.error(
                    SyntaxErrorKind::Invalid,
                    span,
                    format!("module binding `{name}` is already declared"),
                ));
            }
        }
        let imported: BTreeMap<_, _> = self
            .imports
            .iter()
            .map(|i| (i.local_name.as_str(), i))
            .collect();
        let mut local_exports = Vec::new();
        let mut indirect_exports = self.indirect;
        for (local, exported) in self.exports {
            if local.name != "*default*" && !bound.contains_key(local.name.as_str()) {
                return Err(p.error(
                    SyntaxErrorKind::Invalid,
                    local.span,
                    format!(
                        "exported local binding `{}` is not declared in the module",
                        local.name
                    ),
                ));
            }
            if let Some(import) = imported.get(local.name.as_str())
                && let ImportName::Name(name) = &import.import_name
            {
                indirect_exports.push(IndirectExportEntry {
                    specifier: import.specifier.clone(),
                    import_name: name.clone(),
                    export_name: exported.name,
                    span: exported.span,
                });
                continue;
            }
            local_exports.push(LocalExportEntry {
                export_name: exported.name,
                local_name: local.name,
                span: exported.span,
            });
        }
        indirect_exports.sort_by_key(|entry| entry.span.start);
        Ok(ModuleRecord::new(
            body,
            Span::new(0, p.src.len()),
            self.requested,
            self.imports,
            local_exports,
            indirect_exports,
            self.stars,
        ))
    }
}

impl Parser<'_> {
    pub(in crate::frontend) fn parse_module(mut self) -> PResult<Module> {
        self.module_goal = true;
        self.using_allowed = true;
        self.set_strict(true);
        self.advance()?;
        let mut tables = Tables::default();
        let mut body = Vec::new();
        while self.tok.kind != TokenKind::Eof {
            if self.is_keyword(Keyword::Import) {
                self.parse_import_declaration(&mut tables)?;
            } else if self.is_keyword(Keyword::Export) {
                if let Some(item) = self.parse_export_declaration(&mut tables)? {
                    body.push(item);
                }
            } else {
                body.push(ModuleItem::Statement(self.parse_statement_list_item()?));
            }
        }
        if let Some(error) = self.object_cover_errors.iter().min_by_key(|e| e.span.start) {
            return Err(error.clone());
        }
        tables.finish(&self, body)
    }

    fn is_module_word(&self, word: &str) -> bool {
        matches!(&self.tok.kind, TokenKind::Identifier(name) if name == word)
            && self.tok.span.text(self.src) == word
    }

    fn expect_module_word(&mut self, word: &str) -> PResult<()> {
        if !self.is_module_word(word) {
            return Err(self.unexpected(&format!("`{word}`")));
        }
        self.advance()?;
        Ok(())
    }

    fn check_import_binding(&self, ident: &Ident) -> PResult<()> {
        if Keyword::from_text(&ident.name).is_some() {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                ident.span,
                format!(
                    "`{}` is a reserved word and cannot be used as an import binding name",
                    ident.name
                ),
            ));
        }
        if RESERVED_CONSTANTS.contains(&ident.name.as_str())
            || STRICT_RESTRICTED.contains(&ident.name.as_str())
        {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                ident.span,
                format!("`{}` is not a valid import binding name", ident.name),
            ));
        }
        Ok(())
    }

    fn parse_import_binding(&mut self) -> PResult<Ident> {
        let ident = self
            .parse_binding_identifier("an import binding")
            .map_err(|mut e| {
                if e.kind == SyntaxErrorKind::ReservedWord {
                    e.kind = SyntaxErrorKind::Invalid;
                }
                e
            })?;
        self.check_import_binding(&ident)?;
        Ok(ident)
    }

    /// Import/export names are IdentifierNames; aliases may be reserved words.
    /// The lexer is put in that goal before consuming `{`, `,`, or `as`.
    fn parse_module_name(&mut self) -> PResult<Ident> {
        let name = match &self.tok.kind {
            TokenKind::Identifier(name) => name.clone(),
            TokenKind::Keyword(k) => k.as_str().to_owned(),
            TokenKind::String(_) => {
                return Err(
                    self.unsupported(self.tok.span, "string-literal import/export names are")
                );
            }
            _ => return Err(self.unexpected("an import/export name")),
        };
        let span = self.advance()?.span;
        Ok(Ident { name, span })
    }

    fn parse_module_specifier(&mut self) -> PResult<JsString> {
        let TokenKind::String(specifier) = &self.tok.kind else {
            return Err(self.unexpected("a string-literal module specifier"));
        };
        let specifier = specifier.clone();
        self.advance()?;
        if self.is_keyword(Keyword::With) {
            return Err(self.unsupported(self.tok.span, "import attributes are"));
        }
        Ok(specifier)
    }

    fn parse_import_declaration(&mut self, tables: &mut Tables) -> PResult<()> {
        let start = self.tok.span;
        // Lex binding names as IdentifierNames first, so escaped keywords
        // reach the binding check instead of failing during lookahead. Never
        // reclassify a lexical error fetched after the declaration's `;`.
        self.lexer.expect_property_name();
        self.advance()?;
        if self.is_punct(Punct::LParen) {
            return Err(self.unsupported(start, "dynamic `import()` is"));
        }
        if self.is_punct(Punct::Dot) {
            return Err(self.unsupported(start, "`import.meta` is"));
        }
        if matches!(self.tok.kind, TokenKind::String(_)) {
            let specifier = self.parse_module_specifier()?;
            tables.request(&specifier);
            return self.expect_semi();
        }
        let mut bindings = Vec::new();
        if !self.is_punct(Punct::Star) && !self.is_punct(Punct::LBrace) {
            let local = self.parse_import_binding()?;
            bindings.push((ImportName::Name("default".into()), local));
            if !self.eat_punct(Punct::Comma)? {
                self.expect_module_word("from")?;
                let specifier = self.parse_module_specifier()?;
                return self.finish_import(tables, specifier, bindings);
            }
        }
        if self.eat_punct(Punct::Star)? {
            self.lexer.expect_property_name();
            self.expect_module_word("as")?;
            let local = self.parse_import_binding()?;
            bindings.push((ImportName::Namespace, local));
        } else {
            self.lexer.expect_property_name();
            self.expect_punct(Punct::LBrace)?;
            while !self.is_punct(Punct::RBrace) {
                let imported = self.parse_module_name()?;
                let local = if self.is_module_word("as") {
                    self.lexer.expect_property_name();
                    self.advance()?;
                    self.parse_import_binding()?
                } else {
                    self.check_import_binding(&imported)?;
                    imported.clone()
                };
                bindings.push((ImportName::Name(imported.name), local));
                if !self.is_punct(Punct::Comma) {
                    break;
                }
                self.lexer.expect_property_name();
                self.advance()?;
            }
            self.expect_punct(Punct::RBrace)?;
        }
        self.expect_module_word("from")?;
        let specifier = self.parse_module_specifier()?;
        self.finish_import(tables, specifier, bindings)
    }

    fn finish_import(
        &mut self,
        tables: &mut Tables,
        specifier: JsString,
        bindings: Vec<(ImportName, Ident)>,
    ) -> PResult<()> {
        tables.request(&specifier);
        tables.imports.extend(
            bindings
                .into_iter()
                .map(|(import_name, local)| ImportEntry {
                    specifier: specifier.clone(),
                    import_name,
                    local_name: local.name,
                    span: local.span,
                }),
        );
        self.expect_semi()
    }

    fn parse_export_declaration(&mut self, tables: &mut Tables) -> PResult<Option<ModuleItem>> {
        self.advance()?;
        if self.is_keyword(Keyword::Default) {
            return self.parse_default_export(tables).map(Some);
        }
        if self.is_punct(Punct::Star) {
            self.advance()?;
            let namespace = if self.is_module_word("as") {
                self.lexer.expect_property_name();
                self.advance()?;
                Some(self.parse_module_name()?)
            } else {
                None
            };
            if !self.is_module_word("from") {
                return Err(self.unexpected("from"));
            }
            self.advance()?;
            let specifier = self.parse_module_specifier()?;
            tables.request(&specifier);
            if let Some(exported) = namespace {
                if !tables.exported_names.insert(exported.name.clone()) {
                    return Err(self.error(
                        SyntaxErrorKind::Invalid,
                        exported.span,
                        format!("duplicate exported name `{}`", exported.name),
                    ));
                }
                tables.indirect.push(IndirectExportEntry {
                    specifier,
                    import_name: "*namespace*".into(),
                    export_name: exported.name,
                    span: exported.span,
                });
            } else {
                tables.stars.push(specifier);
            }
            self.expect_semi()?;
            return Ok(None);
        }
        if self.is_punct(Punct::LBrace) {
            let export_start = tables.exports.len();
            self.lexer.expect_property_name();
            self.advance()?;
            while !self.is_punct(Punct::RBrace) {
                let local = self.parse_module_name()?;
                let exported = if self.is_module_word("as") {
                    self.lexer.expect_property_name();
                    self.advance()?;
                    self.parse_module_name()?
                } else {
                    local.clone()
                };
                tables.export(self, local, exported)?;
                if !self.is_punct(Punct::Comma) {
                    break;
                }
                self.lexer.expect_property_name();
                self.advance()?;
            }
            self.expect_punct(Punct::RBrace)?;
            if self.is_module_word("from") {
                self.advance()?;
                let specifier = self.parse_module_specifier()?;
                tables.request(&specifier);
                for (imported, exported) in tables.exports.split_off(export_start) {
                    tables.indirect.push(IndirectExportEntry {
                        specifier: specifier.clone(),
                        import_name: imported.name,
                        export_name: exported.name,
                        span: exported.span,
                    });
                }
            }
            self.expect_semi()?;
            return Ok(None);
        }
        if !matches!(
            self.tok.kind,
            TokenKind::Keyword(
                Keyword::Let | Keyword::Const | Keyword::Var | Keyword::Function | Keyword::Class
            )
        ) && !self.at_async_function()
        {
            return Err(self.unexpected("an exported declaration or export list"));
        }
        let stmt = self.parse_statement_list_item()?;
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => {
                let name = f.name.as_ref().expect("a function declaration has a name");
                tables.export(self, name.clone(), name.clone())?;
            }
            StmtKind::Lexical(decl) => {
                for name in decl.declarators.iter().flat_map(|d| d.name.names()) {
                    tables.export(self, name.clone(), name.clone())?;
                }
            }
            StmtKind::Var(decls) => {
                for name in decls.iter().flat_map(|d| d.name.names()) {
                    tables.export(self, name.clone(), name.clone())?;
                }
            }
            _ => unreachable!("export declaration was checked before parsing"),
        }
        Ok(Some(ModuleItem::Statement(stmt)))
    }

    fn parse_default_export(&mut self, tables: &mut Tables) -> PResult<ModuleItem> {
        let exported = Ident {
            name: "default".into(),
            span: self.advance()?.span,
        };
        let value = if self.is_keyword(Keyword::Function) || self.at_async_function() {
            DefaultExport::Function(self.parse_function(false, None)?)
        } else if self.is_keyword(Keyword::Class) {
            let expr = self.parse_class(false)?;
            let ExprKind::Class(class) = expr.kind else {
                unreachable!("parse_class produces a class expression")
            };
            DefaultExport::Class(class)
        } else {
            let expr = self.parse_assignment()?;
            self.expect_semi()?;
            DefaultExport::Expression(expr)
        };
        let local = match &value {
            DefaultExport::Function(f) if f.name.is_some() => f.name.clone().unwrap(),
            DefaultExport::Class(class) if class.name.is_some() => class.name.clone().unwrap(),
            _ => Ident {
                name: "*default*".into(),
                span: exported.span,
            },
        };
        tables.export(self, local, exported)?;
        Ok(ModuleItem::DefaultExport(value))
    }

    /// Only the module's outer loop consumes static declarations.
    pub(super) fn misplaced_module_declaration(&self) -> SyntaxError {
        if self.is_keyword(Keyword::Import) {
            match self.peek_next().map(|t| t.kind) {
                Some(TokenKind::Punct(Punct::LParen)) => {
                    return self.unsupported(self.tok.span, "dynamic `import()` is");
                }
                Some(TokenKind::Punct(Punct::Dot)) => {
                    return self.unsupported(self.tok.span, "`import.meta` is");
                }
                _ => {}
            }
        }
        self.error(
            SyntaxErrorKind::Invalid,
            self.tok.span,
            "import/export declarations are only allowed at the module top level",
        )
    }
}

/// Module VarDeclaredNames, excluding all nested function and lexical scopes.
fn var_declared_names<'a>(stmt: &'a Stmt, out: &mut Vec<&'a Ident>) {
    match &stmt.kind {
        StmtKind::Var(decls) => out.extend(decls.iter().flat_map(|d| d.name.names())),
        StmtKind::For { init, body, .. } => {
            if let Some(ForInit::Var(decls)) = init {
                out.extend(decls.iter().flat_map(|d| d.name.names()));
            }
            var_declared_names(body, out);
        }
        StmtKind::ForIn { head, body, .. } | StmtKind::ForOf { head, body, .. } => {
            if let ForInHead::Var(name) = head {
                out.push(name);
            }
            var_declared_names(body, out);
        }
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            var_declared_names(consequent, out);
            if let Some(stmt) = alternate {
                var_declared_names(stmt, out);
            }
        }
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::Labelled { body, .. } => var_declared_names(body, out),
        StmtKind::Try {
            block,
            handler,
            finalizer,
        } => {
            for stmt in block
                .iter()
                .chain(handler.iter().flat_map(|h| &h.body))
                .chain(finalizer.iter().flatten())
            {
                var_declared_names(stmt, out);
            }
        }
        StmtKind::Block(body) => {
            for stmt in body {
                var_declared_names(stmt, out);
            }
        }
        StmtKind::Switch { cases, .. } => {
            for stmt in cases.iter().flat_map(|c| &c.body) {
                var_declared_names(stmt, out);
            }
        }
        _ => {}
    }
}
