use std::env;
use std::path::PathBuf;

fn main() {
    let c_src_dir = "verilog-parser/src";

    println!("cargo:rerun-if-changed={}", c_src_dir);

    cc::Build::new()
        .files([
            format!("{}/verilog_ast.c",c_src_dir),
            format!("{}/verilog_ast_common.c",c_src_dir),
            format!("{}/verilog_ast_mem.c", c_src_dir),
            format!("{}/verilog_ast_util.c", c_src_dir),
            format!("{}/verilog_parser_wrapper.c", c_src_dir),
            format!("{}/verilog_preprocessor.c", c_src_dir),
            format!("{}/build/verilog_parser.tab.c", c_src_dir),
            format!("{}/build/verilog_scanner.c", c_src_dir),
        ])
        .include(c_src_dir)
        .warnings(false)
        .compile("verilog_parser");

    let bindings = bindgen::Builder::default()
        .header(format!("{}/verilog_parser.h", c_src_dir))
        .header(format!("{}/verilog_ast_mem.h", c_src_dir))
        .clang_arg(format!("-I{}", c_src_dir))
        .allowlist_function("verilog_parser_init")
        .allowlist_function("verilog_parse_file")
        .allowlist_function("ast_free_all")
        .allowlist_function("ast_list_get")

        .allowlist_var("yy_verilog_source_tree")

        .allowlist_type("verilog_source_tree")
        .allowlist_type("ast_module_declaration")
        .allowlist_type("ast_port_declaration")
        .allowlist_type("ast_port_direction")
        .allowlist_type("ast_net_type")
        .allowlist_type("ast_range")
        .allowlist_type("ast_expression")
        .allowlist_type("ast_identifier_t")
        .allowlist_type("ast_list")
        .allowlist_type("ast_number")

        .blocklist_type("FILE")
        .blocklist_type("_IO_FILE")
        .blocklist_type("_IO_marker")
        .blocklist_type("_IO_codecvt")
        .blocklist_type("_IO_wide_data")

        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

}