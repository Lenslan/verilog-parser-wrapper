#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::redundant_static_lifetimes)]

use std::collections::HashMap;
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
            fn from_c_net_type(net_type: ast_net_type) -> Self {
                match net_type {
                    ast_net_type_e_NET_TYPE_WIRE => SignalType::Wire,
                    ast_net_type_e_NET_TYPE_TRI => SignalType::Tri,
                    ast_net_type_e_NET_TYPE_TRIREG => SignalType::Reg,
                    // 默认是 wire
                    _ => SignalType::Unknown,
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

// 用于存储 net/reg 详细信息的临时结构
#[derive(Debug, Clone)]
struct SignalDetail {
    signal_type: SignalType,
    msb: Option<i32>,
    lsb: Option<i32>,
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

        // 初始化解析器
        unsafe { verilog_parser_init() };

        // **重要修复：设置预处理器上下文**
        unsafe {
            // 1. 设置 include 搜索路径 (可选但推荐)
            // let current_dir = CString::new("./").unwrap();
            // ast_list_append((*yy_preproc).search_dirs, current_dir.as_ptr() as *mut _);
            //
            // if let Some(parent) = filepath.parent() {
            //     if let Some(parent_str) = parent.to_str(){
            //         if !parent_str.is_empty() {
            //             let parent_dir = CString::new(parent_str).unwrap();
            //             ast_list_append((*yy_preproc).search_dirs, parent_dir.as_ptr() as *mut _);
            //         }
            //     }
            // }

            // 2. 设置当前解析的文件名 (修复宏定义SIGSEGV的关键)
            verilog_preprocessor_set_file(yy_preproc, c_filepath.as_ptr() as *mut _);
        }

        // 解析文件
        let result = unsafe { verilog_parse_file(file_ptr) };
        unsafe { fclose(file_ptr) };

        if result != 0 {
            unsafe { ast_free_all() };
            return Err("Failed to parse Verilog syntax".to_string());
        }

        unsafe { verilog_resolve_modules(yy_verilog_source_tree) };

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

    /// 从 ast_module_declaration 节点提取单个模块信息 (已重构)
    fn extract_module_data(module_decl_ptr: *mut ast_module_declaration) -> Option<ModuleInfo> {
        if module_decl_ptr.is_null() { return None; }
        let module_decl = unsafe { &*module_decl_ptr };

        let name = unsafe {
            module_decl.identifier.as_ref()
                .map(|id| CStr::from_ptr(id.identifier).to_string_lossy().into_owned())
                .unwrap_or_default()
        };

        // **修复第一步：创建信号详情的查找表**
        let mut signal_details = HashMap::new();

        // 从 net 声明中提取
        let net_decls_ptr = module_decl.net_declarations;
        if !net_decls_ptr.is_null() {
            let num_nets = unsafe { (*net_decls_ptr).items };
            for i in 0..num_nets {
                let net_decl_ptr = unsafe { ast_list_get(net_decls_ptr, i) as *mut ast_net_declaration };
                if !net_decl_ptr.is_null() {
                    let net_decl = unsafe { &*net_decl_ptr };
                    let (msb, lsb) = Self::extract_range(net_decl.range);
                    let detail = SignalDetail {
                        signal_type: SignalType::from_c_net_type(net_decl.type_),
                        msb,
                        lsb,
                    };
                    if let Some(ident) = unsafe { net_decl.identifier.as_ref() } {
                        let signal_name = unsafe { CStr::from_ptr(ident.identifier).to_string_lossy().into_owned() };
                        signal_details.insert(signal_name, detail);
                    }
                }
            }
        }

        // 从 reg 声明中提取
        let reg_decls_ptr = module_decl.reg_declarations;
        if !reg_decls_ptr.is_null() {
            let num_regs = unsafe { (*reg_decls_ptr).items };
            for i in 0..num_regs {
                let reg_decl_ptr = unsafe { ast_list_get(reg_decls_ptr, i) as *mut ast_reg_declaration };
                if !reg_decl_ptr.is_null() {
                    let reg_decl = unsafe { &*reg_decl_ptr };
                    let (msb, lsb) = Self::extract_range(reg_decl.range);
                    let detail = SignalDetail {
                        signal_type: SignalType::Reg,
                        msb,
                        lsb,
                    };
                    if let Some(ident) = unsafe { reg_decl.identifier.as_ref() } {
                        let signal_name = unsafe { CStr::from_ptr(ident.identifier).to_string_lossy().into_owned() };
                        signal_details.insert(signal_name, detail);
                    }
                }
            }
        }


        // **修复第二步：遍历端口并使用查找表**
        let mut ports = Vec::new();
        let ports_list_ptr = module_decl.module_ports;
        if !ports_list_ptr.is_null() {
            let num_ports_decls = unsafe { (*ports_list_ptr).items };
            for i in 0..num_ports_decls {
                let port_decl_ptr = unsafe { ast_list_get(ports_list_ptr, i) as *mut ast_port_declaration };
                if !port_decl_ptr.is_null() {
                    let port_decl = unsafe { &*port_decl_ptr };
                    let direction = PortDirection::from(port_decl.direction);

                    let names_list_ptr = port_decl.port_names;
                    if !names_list_ptr.is_null() {
                        let num_names = unsafe { (*names_list_ptr).items };
                        for j in 0..num_names {
                            let ident_ptr = unsafe { ast_list_get(names_list_ptr, j) as *mut ast_identifier_t };
                            if !ident_ptr.is_null() {
                                let ident = unsafe { &*ident_ptr };
                                let port_name = unsafe { CStr::from_ptr(ident.identifier).to_string_lossy().into_owned() };

                                // 从查找表中获取详细信息
                                let detail = signal_details.get(&port_name);

                                // 即使 port_decl.range 为空，也能从 detail 中获取位宽
                                let (mut msb, mut lsb) = Self::extract_range(port_decl.range);
                                if msb.is_none() {
                                    msb = detail.and_then(|d| d.msb);
                                    lsb = detail.and_then(|d| d.lsb);
                                }

                                let signal_type = detail.map(|d| d.signal_type.clone()).unwrap_or(SignalType::Wire);

                                ports.push(PortInfo {
                                    name: port_name,
                                    direction: direction.clone(),
                                    signal_type,
                                    msb,
                                    lsb,
                                });
                            }
                        }
                    }
                }
            }
        }

        Some(ModuleInfo { name, ports })
    }

    /// 辅助函数：从 ast_range 提取位宽
    fn extract_range(range_ptr: *mut ast_range) -> (Option<i32>, Option<i32>) {
        if range_ptr.is_null() {
            println!(">> there is no range");
            return (None, None);
        }
        let range = unsafe { &*range_ptr };
        (
            Self::extract_expression_as_i32(range.upper),
            Self::extract_expression_as_i32(range.lower)
        )
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
                    // Verilog 数字可能包含 'b, 'h, 'd, 'o 等，这里简化处理
                    let clean_s = s.split('\'').last().unwrap_or(&s);
                    return clean_s.parse::<i32>().ok();
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
    fn test_width() -> Result<(), String> {
        let result = add(2, 2);
        assert_eq!(result, 4);

        let ast = VerilogAST::parse_file(Path::new("tests/std-7.1.6-primitives.v"))?;
        for module in ast {
            println!("===>module name is {}", module.name);
            for port in module.ports {
                println!("===>module port info :{:?}", port);
            }

        }

        Ok(())
    }

    // #[test]
    // fn test_macros() -> Result<(), String> {
    //     let result = add(2, 2);
    //     assert_eq!(result, 4);
    //
    //     let ast = VerilogAST::parse_file(Path::new("tests/macros.v"))?;
    //     for module in ast {
    //         println!("===>module name is {}", module.name);
    //         for port in module.ports {
    //             println!("===>module port info :{:?}", port);
    //         }
    //
    //     }
    //
    //     Ok(())
    // }
}