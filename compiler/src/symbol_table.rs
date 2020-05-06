use crate::error::{CompileError, CompileErrorType};
use indexmap::map::IndexMap;
use std::ops::Add;
use zoker_parser::ast;
use zoker_parser::location::Location;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SymbolType {
    Unknown,
    Contract,
    Function,
    Uint256,
    Int256,
    String,
    Address,
    Bytes32,
    Bytes,
    Bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolUsage {
    Used,
    Declared,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SymbolTableType {
    Global,
    Contract,
    Function,
    Local,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolLocation {
    Unknown,
    Storage,
    Memory,
}

#[derive(Debug, PartialEq)]
pub struct SymbolTableError {
    pub error: String,
    pub location: Location,
}

impl From<SymbolTableError> for CompileError {
    fn from(error: SymbolTableError) -> Self {
        CompileError {
            error: CompileErrorType::SyntaxError(error.error),
            location: error.location,
        }
    }
}

type SymbolTableResult = Result<(), SymbolTableError>;

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub symbol_type: SymbolType,
    pub data_location: SymbolLocation,
    pub role: SymbolUsage,
}

impl Symbol {
    fn new(
        name: String,
        role: SymbolUsage,
        symbol_type: SymbolType,
        data_location: SymbolLocation,
    ) -> Self {
        Symbol {
            name,
            symbol_type,
            data_location,
            role,
        }
    }
}

#[derive(Clone)]
pub struct SymbolTable {
    pub name: String,
    pub table_type: SymbolTableType,
    pub symbols: IndexMap<String, Symbol>,
    pub sub_tables: Vec<SymbolTable>,
}

#[derive(Default)]
struct SymbolTableBuilder {
    pub if_num: Vec<u32>,
    pub for_num: Vec<u32>,
    pub compound_num: Vec<u32>,
    pub tables: Vec<SymbolTable>,
}

#[derive(Default)]
struct SymbolAnalyzer {
    tables: Vec<AnalysisTable>,
}

struct AnalysisTable {
    pub map: IndexMap<String, Symbol>,
    pub typ: SymbolTableType,
}

impl AnalysisTable {
    fn new(map: IndexMap<String, Symbol>, typ: SymbolTableType) -> Self {
        AnalysisTable { map, typ }
    }
}

pub fn make_symbol_tables(program: &ast::Program) -> Result<SymbolTable, SymbolTableError> {
    SymbolTableBuilder::new().prepare_table(program)?.build()
}

impl SymbolAnalyzer {
    fn analyze_symbol_table(&mut self, table: &SymbolTable) -> SymbolTableResult {
        let sub_tables = &table.sub_tables;

        self.tables
            .push(AnalysisTable::new(table.symbols.clone(), table.table_type));

        for sub_table in sub_tables {
            self.analyze_symbol_table(sub_table)?;
        }
        let mut analysis_table = self.tables.pop().unwrap();

        for value in analysis_table.map.values_mut() {
            self.analyze_symbol(value)?;
        }
        Ok(())
    }

    fn analyze_symbol(&mut self, symbol: &Symbol) -> SymbolTableResult {
        match symbol.role {
            SymbolUsage::Declared => {
                // No need to do anything.
            }
            SymbolUsage::Used => {
                let is_declared = self.tables.iter().any(|table| {
                    if let Some(sym) = table.map.get(&symbol.name) {
                        sym.role != SymbolUsage::Used
                    } else {
                        false
                    }
                });

                if !is_declared {
                    return Err(SymbolTableError {
                        error: format!("Variable {} is not declared, but used.", symbol.name),
                        location: Default::default(),
                    });
                }
            }
        }
        Ok(())
    }
}

impl SymbolTableBuilder {
    fn new() -> Self {
        SymbolTableBuilder {
            if_num: vec![],
            for_num: vec![],
            compound_num: vec![],
            tables: vec![],
        }
    }

    fn prepare_table(mut self, program: &ast::Program) -> Result<Self, SymbolTableError> {
        self.enter_program(program)?;
        Ok(self)
    }

    fn enter_scope(&mut self, name: String, table_type: SymbolTableType) {
        self.if_num.push(0);
        self.for_num.push(0);
        self.compound_num.push(0);
        self.tables.push(SymbolTable {
            name,
            table_type,
            symbols: Default::default(),
            sub_tables: vec![],
        });
    }

    fn exit_scope(&mut self) {
        self.if_num.pop();
        self.for_num.pop();
        self.compound_num.pop();
        let table = self.tables.pop().unwrap();
        self.tables.last_mut().unwrap().sub_tables.push(table);
    }

    fn enter_program(&mut self, program: &ast::Program) -> SymbolTableResult {
        self.enter_scope(String::from("#Global"), SymbolTableType::Global);
        match program {
            ast::Program::GlobalStatements(stmts) => {
                self.enter_global_statements(stmts)?;
            }
        }
        Ok(())
    }

    fn enter_global_statements(&mut self, statements: &[ast::Statement]) -> SymbolTableResult {
        for stmt in statements {
            self.enter_statement(stmt, &SymbolLocation::Memory)?;
        }
        Ok(())
    }

    fn enter_block(
        &mut self,
        compound: &ast::Statement,
        location: &SymbolLocation,
    ) -> SymbolTableResult {
        if let ast::StatementType::CompoundStatement {
            statements,
            return_value,
        } = &compound.node
        {
            for stmt in statements {
                self.enter_statement(stmt, location)?;
            }
            if let Some(returns) = return_value {
                self.enter_expression(returns)?;
            }
        }
        Ok(())
    }

    fn enter_statement(
        &mut self,
        statement: &ast::Statement,
        location: &SymbolLocation,
    ) -> SymbolTableResult {
        match &statement.node {
            ast::StatementType::Expression { expression: expr } => self.enter_expression(expr)?,
            ast::StatementType::FunctionStatement {
                function_name: func,
                parameters: params,
                statement: stmt,
            } => {
                let name = func.node.identifier_name().unwrap();
                let tables = self.tables.last_mut().unwrap();
                let symbol = Symbol::new(
                    name.clone(),
                    SymbolUsage::Declared,
                    SymbolType::Function,
                    SymbolLocation::Storage,
                );
                tables.symbols.insert(name.clone(), symbol);

                self.enter_scope(name, SymbolTableType::Function);
                self.enter_expression(params)?;
                self.enter_block(stmt, &SymbolLocation::Unknown)?;
                self.exit_scope();
            }
            ast::StatementType::ContractStatement {
                contract_name: name,
                members: stmts,
            } => {
                let name = name.node.identifier_name().unwrap();
                let tables = self.tables.last_mut().unwrap();
                let symbol = Symbol::new(
                    name.clone(),
                    SymbolUsage::Declared,
                    SymbolType::Contract,
                    SymbolLocation::Storage,
                );
                tables.symbols.insert(name.clone(), symbol);

                self.enter_scope(name, SymbolTableType::Contract);
                self.enter_statement(stmts, location)?;
                self.exit_scope();
            }
            ast::StatementType::InitializerStatement {
                variable_type,
                data_location: loc,
                variable: var,
                default,
            } => {
                if let Some(data_location) = loc {
                    let data_location = match data_location {
                        ast::Specifier::Storage => SymbolLocation::Storage,
                        ast::Specifier::Memory => SymbolLocation::Memory,
                    };
                    self.register_identifier(var, variable_type, &data_location);
                } else {
                    self.register_identifier(var, variable_type, location);
                }
                if let Some(expr) = default {
                    self.enter_expression(expr)?;
                }
            }
            ast::StatementType::CompoundStatement {
                statements: stmts,
                return_value: returns,
            } => {
                let number = self.compound_num.last_mut().unwrap();
                *number += 1;
                let name = String::from("#Compound_").add(&*(number).to_string());
                self.enter_scope(name, SymbolTableType::Local);
                for stmt in stmts {
                    self.enter_statement(stmt, location)?;
                }
                if let Some(expr) = returns {
                    self.enter_expression(expr)?;
                }
                self.exit_scope();
            }
            ast::StatementType::MemberStatement {
                statements: members,
            } => {
                for member in members {
                    self.enter_statement(member, &SymbolLocation::Storage)?;
                }
            }
        }
        Ok(())
    }

    fn enter_expression(&mut self, expression: &ast::Expression) -> SymbolTableResult {
        match &expression.node {
            ast::ExpressionType::AssignExpression { left, right, .. } => {
                self.enter_expression(left)?;
                self.enter_expression(right)?;
            }
            ast::ExpressionType::TernaryExpression {
                condition,
                expr1,
                expr2,
            } => {
                self.enter_expression(condition)?;
                self.enter_expression(expr1)?;
                self.enter_expression(expr2)?;
            }
            ast::ExpressionType::BinaryExpression { left, right, .. } => {
                self.enter_expression(left)?;
                self.enter_expression(right)?;
            }
            ast::ExpressionType::FunctionCallExpression {
                function_name,
                arguments,
            } => {
                self.enter_expression(function_name)?;
                self.enter_expression(arguments)?;
            }
            ast::ExpressionType::IfExpression {
                condition,
                if_statement,
                else_statement,
            } => {
                self.enter_expression(condition)?;
                let if_num = self.if_num.last_mut().unwrap();
                *if_num += 1;
                let if_name = String::from("#If_").add(&*(if_num).to_string());
                let else_name = String::from("#Else_").add(&*(if_num).to_string());
                self.enter_scope(if_name, SymbolTableType::Local);
                self.enter_block(if_statement, &SymbolLocation::Unknown)?;
                self.exit_scope();

                if let Some(expr) = else_statement {
                    self.enter_scope(else_name, SymbolTableType::Local);
                    self.enter_block(expr, &SymbolLocation::Unknown)?;
                    self.exit_scope();
                }
            }
            ast::ExpressionType::ForEachExpression {
                iterator,
                vector,
                statement,
                else_statement,
            } => {
                self.check_identifier(vector);
                let for_num = self.for_num.last_mut().unwrap();
                *for_num += 1;
                let for_name = String::from("#For_").add(&*(for_num).to_string());
                let else_name = String::from("#Else_").add(&*(for_num).to_string());
                self.enter_scope(for_name, SymbolTableType::Local);
                self.enter_expression(iterator)?;
                self.enter_block(statement, &SymbolLocation::Unknown)?;
                self.exit_scope();
                if let Some(stmt) = else_statement {
                    self.enter_scope(else_name, SymbolTableType::Local);
                    self.enter_block(stmt, &SymbolLocation::Unknown)?;
                    self.exit_scope();
                }
            }
            ast::ExpressionType::UnaryExpression { expression, .. } => {
                self.enter_expression(expression)?;
            }
            ast::ExpressionType::Parameters { parameters: params } => {
                for param in params {
                    self.enter_statement(param, &SymbolLocation::Unknown)?;
                }
            }
            ast::ExpressionType::Arguments { arguments: args } => {
                for arg in args {
                    self.enter_expression(arg)?;
                }
            }
            ast::ExpressionType::Number { .. } => {}
            ast::ExpressionType::Identifier { .. } => {
                self.check_identifier(expression);
            }
        }
        Ok(())
    }

    fn check_identifier(&mut self, identifier: &ast::Expression) {
        let name = identifier.node.identifier_name().unwrap();
        let tables = self.tables.last_mut().unwrap();
        if tables.symbols.get(&name).is_none() {
            let symbol = Symbol::new(
                name.clone(),
                SymbolUsage::Used,
                SymbolType::Unknown,
                SymbolLocation::Unknown,
            );
            tables.symbols.insert(name, symbol);
        } else {
            // TODO: Check Declared Variable?
        }
    }

    fn register_identifier(
        &mut self,
        expr: &ast::Expression,
        typ: &ast::Type,
        loc: &SymbolLocation,
    ) {
        let name = expr.node.identifier_name().unwrap();
        // TODO: Check for symbol already in table.
        let symbol_type = match typ {
            ast::Type::String => SymbolType::String,
            ast::Type::Uint256 => SymbolType::Uint256,
            ast::Type::Int256 => SymbolType::Int256,
            ast::Type::Bytes32 => SymbolType::Bytes32,
            ast::Type::Bool => SymbolType::Bool,
            ast::Type::Bytes => SymbolType::Bytes,
            ast::Type::Address => SymbolType::Address,
        };
        let data_location = if loc != &SymbolLocation::Unknown {
            loc.clone()
        } else {
            self.default_location(symbol_type)
        };
        let symbol = Symbol::new(
            name.clone(),
            SymbolUsage::Declared,
            symbol_type,
            data_location,
        );
        let tables = self.tables.last_mut().unwrap();
        tables.symbols.insert(name, symbol);
    }

    fn default_location(&self, typ: SymbolType) -> SymbolLocation {
        match typ {
            SymbolType::Unknown => SymbolLocation::Unknown,
            SymbolType::Contract => SymbolLocation::Storage,
            SymbolType::Function => SymbolLocation::Storage,
            SymbolType::Uint256 => SymbolLocation::Memory,
            SymbolType::Int256 => SymbolLocation::Memory,
            SymbolType::String => SymbolLocation::Storage,
            SymbolType::Address => SymbolLocation::Memory,
            SymbolType::Bytes32 => SymbolLocation::Storage,
            SymbolType::Bytes => SymbolLocation::Storage,
            SymbolType::Bool => SymbolLocation::Memory,
        }
    }

    fn build(mut self) -> Result<SymbolTable, SymbolTableError> {
        let table = self.tables.pop().unwrap();
        let mut analyzer = SymbolAnalyzer::default();
        analyzer.analyze_symbol_table(&table)?;
        Ok(table)
    }
}
