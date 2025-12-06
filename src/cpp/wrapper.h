#pragma once

#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// Opaque pointer to the flatbuffers::Parser
struct FlatbuffersParser;

struct Position {
    unsigned line, col; // 0-based
};
struct Range {
    struct Position start, end; // 0-based
};

// A struct to pass struct/table definition information
struct StructDefinitionInfo {
    const char* name;
    const char* file;
    const char* namespace_;
    const char* documentation;
    bool is_table;
    unsigned line;
    unsigned col;
    size_t bytesize; // struct only
    size_t minalign; // struct only
};

// A struct to pass enum/union definition information
struct EnumDefinitionInfo {
    const char* name;
    const char* file;
    const char* namespace_;
    const char* documentation;
    const char* underlying_type;
    bool is_union;
    unsigned line;
    unsigned col;
};

// A struct to pass enum value information
struct EnumValDefinitionInfo {
    const char* name;
    const char* documentation;
    long long value;
    unsigned line;
    unsigned col;
    struct Range type_range;
    const char* type_source; // text of the type declaration
};

// A struct to pass field information
struct FieldDefinitionInfo {
    const char* name;
    const char* type_name;      // fully qualified display name, including vectory/array symbols
    const char* base_type_name; // fully qualified name of the type or the vector/array's element type
    const char* documentation;
    unsigned line;
    unsigned col;
    struct Range type_range;
    const char* type_source; // text of the type declaration
    bool deprecated;
    bool has_id;
    int id;
};

struct RootTypeDefinitionInfo {
    const char* name; // fully-qualified type name
    const char* file;
    struct Range type_range;
    const char* type_source; // text of the type declaration
};

struct RpcServiceDefinitionInfo {
    const char* name;
    const char* file;
    const char* namespace_;
    const char* documentation;
    unsigned line;
    unsigned col;
};

struct RpcMethodDefinitionInfo {
    const char* name;
    const char* documentation;
    unsigned line;
    unsigned col;
    const char* request_type_name; // fully qualified name of the type
    struct Range request_range;
    const char* request_source; // text of the type declaration
    const char* response_type_name; // fully qualified name of the type
    struct Range response_range;
    const char* response_source; // text of the type declaration
};

// Parses a schema and returns a pointer to the Parser object.
struct FlatbuffersParser* parse_schema(const char* schema_content, const char* filename, const char **include_paths);

// Deletes a parser object.
void delete_parser(struct FlatbuffersParser* parser);

// Returns the error string from the parser.
const char* get_parser_error(struct FlatbuffersParser* parser);

// Returns true if the parser has no errors.
bool is_parser_success(struct FlatbuffersParser* parser);

// Functions for structs and tables
int get_num_structs(struct FlatbuffersParser* parser);
struct StructDefinitionInfo get_struct_info(struct FlatbuffersParser* parser, int index);

// Functions for enums and unions
int get_num_enums(struct FlatbuffersParser* parser);
struct EnumDefinitionInfo get_enum_info(struct FlatbuffersParser* parser, int index);

// Functions for enum values
int get_num_enum_vals(struct FlatbuffersParser* parser, int enum_index);
struct EnumValDefinitionInfo get_enum_val_info(struct FlatbuffersParser* parser, int enum_index, int val_index);

// Functions for root type
bool has_root_type(struct FlatbuffersParser* parser);
struct RootTypeDefinitionInfo get_root_type_info(struct FlatbuffersParser* parser);

// Functions for fields
int get_num_fields(struct FlatbuffersParser* parser, int struct_index);
struct FieldDefinitionInfo get_field_info(struct FlatbuffersParser* parser, int struct_index, int field_index);

// Functions for RPC services
int get_num_rpc_services(struct FlatbuffersParser* parser);
struct RpcServiceDefinitionInfo get_rpc_service_info(struct FlatbuffersParser* parser, int index);

// Functions for RPC methods
int get_num_rpc_methods(struct FlatbuffersParser* parser, int service_index);
struct RpcMethodDefinitionInfo get_rpc_method_info(struct FlatbuffersParser* parser, int service_index, int method_index);

// Functions for user-defined attributes
int get_num_user_defined_attributes(struct FlatbuffersParser* parser);
const char* get_user_defined_attribute(struct FlatbuffersParser* parser, int index);
const char* get_user_defined_attribute_doc(struct FlatbuffersParser* parser, const char* name);

// Functions for all included files
int get_num_all_included_files(struct FlatbuffersParser* parser);
const char* get_all_included_file_path(struct FlatbuffersParser* parser, int index);

// Functions for include graph
int get_num_files_with_includes(struct FlatbuffersParser* parser);
const char* get_file_with_includes_path(struct FlatbuffersParser* parser, int index);
int get_num_includes_for_file(struct FlatbuffersParser* parser, const char* file_path);
const char* get_included_file_path(struct FlatbuffersParser* parser, const char* file_path, int index);


#ifdef __cplusplus
}
#endif
