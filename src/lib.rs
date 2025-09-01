#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::redundant_static_lifetimes)]

use std::ffi::{CStr, CString};
use std::path::Path;
use libc::{fopen, fclose, FILE};


include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortDirection {
    Input,
    Output,
    Inout,
    Unknown,
}

impl From<ast_port_direction> for PortDirection {
    fn from(value: ast_port_direction) -> Self {
        match value {
            ast_port_direction_e_PORT_INPUT => PortDirection::Input,
            ast_port_direction_e_PORT_OUTPUT => PortDirection::Output,
            ast_port_direction_e_PORT_INOUT => PortDirection::Inout,
            _ => PortDirection::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalType {
    Wire,
    Reg,
    Tri,
    // 添加其他需要的类型
    Unknown,
}

impl SignalType {
    fn from_c(net_type: ast_net_type, is_reg: ast_boolean) -> Self {
        if is_reg == ast_boolean_e_AST_TRUE {
            return SignalType::Reg;
        }
        match net_type {
            ast_net_type_e_NET_TYPE_WIRE => SignalType::Wire,
            ast_net_type_e_NET_TYPE_TRI => SignalType::Tri,
            ast_net_type_e_NET_TYPE_TRIREG => SignalType::Reg, // trireg 也是一种 reg
            _ => SignalType::Unknown, // Verilog 默认是 wire
        }
    }
}

#[derive(Debug, Clone)]
pub struct PortInfo {
    pub name: String,
    pub direction: PortDirection,
    pub signal_type: SignalType,
    pub msb: Option<i32>,
    pub lsb: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub ports: Vec<PortInfo>,
}

pub struct VerilogAST;

impl VerilogAST {
    pub fn parse_file(filepath: &Path) -> Result<Vec<ModuleInfo>, String> {
        let c_filepath = CString::new(filepath.to_str().ok_or("Invalid filepath string")?)
            .map_err(|e| e.to_string())?;
        let c_mode = CString::new("r").unwrap();

        let file_ptr: *mut FILE = unsafe { fopen(c_filepath.as_ptr(), c_mode.as_ptr()) };
        if file_ptr.is_null() {
            return Err(format!("Failed to open file: {}", filepath.display()));
        }

        // 初始化解析器，这会创建 yy_verilog_source_tree
        unsafe { verilog_parser_init() };

        // 解析文件
        let result = unsafe { verilog_parse_file(file_ptr) };
        unsafe { fclose(file_ptr) };

        if result != 0 {
            // 解析失败后，需要手动释放内存
            unsafe { ast_free_all() };
            return Err("Failed to parse Verilog syntax".to_string());
        }

        // 从全局变量 yy_verilog_source_tree 中提取数据
        let modules = Self::extract_modules_from_ast();

        // 释放 C AST 内存
        unsafe { ast_free_all() };

        Ok(modules)
    }

    fn extract_modules_from_ast() -> Vec<ModuleInfo> {
        let mut modules = Vec::new();
        let source_tree_ptr = unsafe { yy_verilog_source_tree };
        if source_tree_ptr.is_null() {
            return modules;
        }

        let source_tree = unsafe { &*source_tree_ptr };
        let modules_list_ptr = source_tree.modules;
        if modules_list_ptr.is_null() {
            return modules;
        }

        let num_modules = unsafe { (*modules_list_ptr).items };
        for i in 0..num_modules {
            let module_decl_ptr = unsafe { ast_list_get(modules_list_ptr, i) } as *mut ast_module_declaration;
            if let Some(module_info) = Self::extract_module_data(module_decl_ptr) {
                modules.push(module_info);
            }
        }

        modules
    }

    /// 从 ast_module_declaration 节点提取单个模块信息
    fn extract_module_data(module_decl_ptr: *mut ast_module_declaration) -> Option<ModuleInfo> {
        if module_decl_ptr.is_null() { return None; }
        let module_decl = unsafe { &*module_decl_ptr };

        let name = unsafe { CStr::from_ptr(module_decl.identifier.as_ref().unwrap().identifier).to_string_lossy().into_owned() };
        let mut ports = Vec::new();

        let ports_list_ptr = module_decl.module_ports;
        if !ports_list_ptr.is_null() {
            let num_ports_decls = unsafe { (*ports_list_ptr).items };
            for i in 0..num_ports_decls {
                let port_decl_ptr = unsafe { ast_list_get(ports_list_ptr, i) } as *mut ast_port_declaration;
                ports.append(&mut Self::extract_port_info(port_decl_ptr));
            }
        }

        Some(ModuleInfo { name, ports })
    }

    /// 从 ast_port_declaration 节点提取一个或多个端口信息
    fn extract_port_info(port_decl_ptr: *mut ast_port_declaration) -> Vec<PortInfo> {
        if port_decl_ptr.is_null() { return vec![]; }
        let port_decl = unsafe { &*port_decl_ptr };

        let direction = PortDirection::from(port_decl.direction);
        let signal_type = SignalType::from_c(port_decl.net_type, port_decl.is_reg);

        let (msb, lsb) = if !port_decl.range.is_null() {
            let range = unsafe { &*port_decl.range };
            (
                Self::extract_expression_as_i32(range.upper),
                Self::extract_expression_as_i32(range.lower)
            )
        } else {
            (None, None)
        };

        let mut ports = Vec::new();
        let names_list_ptr = port_decl.port_names;
        if !names_list_ptr.is_null() {
            let num_names = unsafe { (*names_list_ptr).items };
            for i in 0..num_names {
                let ident_ptr = unsafe { ast_list_get(names_list_ptr, i) as *mut ast_identifier_t };
                if !ident_ptr.is_null() {
                    let ident = unsafe { &*ident_ptr };
                    let name = unsafe { CStr::from_ptr(ident.identifier).to_string_lossy().into_owned() };
                    ports.push(PortInfo {
                        name,
                        direction: direction.clone(),
                        signal_type: signal_type.clone(),
                        msb,
                        lsb,
                    });
                }
            }
        }
        ports
    }

    /// 从 ast_expression 中提取整数值
    fn extract_expression_as_i32(expr_ptr: *mut ast_expression) -> Option<i32> {
        if expr_ptr.is_null() { return None; }
        let expr = unsafe { &*expr_ptr };

        // 简化处理：只处理最简单的数字常量表达式
        if expr.type_ == ast_expression_type_e_PRIMARY_EXPRESSION {
            let primary = unsafe { expr.primary.as_ref() }?;
            if primary.value_type == ast_primary_value_type_e_PRIMARY_NUMBER {
                let number = unsafe { primary.value.number.as_ref() }?;
                if number.representation == ast_number_representation_e_REP_INTEGER {
                    return Some(unsafe { number.__bindgen_anon_1.as_int });
                } else if number.representation == ast_number_representation_e_REP_BITS {
                    // 对于位字符串，尝试解析它
                    let s = unsafe { CStr::from_ptr(number.__bindgen_anon_1.as_bits).to_string_lossy() };
                    return s.parse::<i32>().ok();
                }
            }
        }
        None
    }

}



pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() -> Result<(), String> {
        let result = add(2, 2);
        assert_eq!(result, 4);

        let ast = VerilogAST::parse_file(&Path::new("tests/std-7.1.6-primitives.v"))?;
        let ast = VerilogAST::parse_file(&Path::new("tests/macros.v"))?;
        for module in ast {
            println!("module name is : {}", module.name);
            for port in module.ports {
                println!("Ports messages are: {:?}", port)
            }
            println!("module over ----");
        }
        // assert_eq!(ast[0].name, String::from("bus_snooper"), " module name is unequal");
        // assert_eq!(ast[0].ports[0].name, String::from("safe_buslines"), "port name is unequal");
        // assert_eq!(ast[0].ports[0].lsb, Some(0), "port lsb is unequal");
        // assert_eq!(ast[0].ports[0].msb, Some(31), "port msb is unequal");
        // assert_eq!(ast[0].ports[1].name, String::from("bus_lines_1"), "port2 name is unequal");
        // assert_eq!(ast[0].ports[1].direction, PortDirection::Input, "port direction is unequal");



        Ok(())
    }
}
