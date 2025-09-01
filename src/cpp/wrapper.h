#pragma once

#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// Opaque pointer to the flatbuffers::Parser
struct FlatbuffersParser;

// A struct to pass struct/table definition information
struct StructDefinitionInfo {
    const char* name;
    const char* file;
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
    bool is_union;
    unsigned line;
    unsigned col;
};

// A struct to pass enum value information
struct EnumValDefinitionInfo {
    const char* name;
    long long value;
    unsigned line;
    unsigned col;
};

// A struct to pass field information
struct FieldDefinitionInfo {
    const char* name;
    unsigned line;
    unsigned col;
    unsigned type_line;
    unsigned type_col;
};

// Parses a schema and returns a pointer to the Parser object.
struct FlatbuffersParser* parse_schema(const char* schema_content, const char* filename);

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

// Functions for fields
int get_num_fields(struct FlatbuffersParser* parser, int struct_index);
struct FieldDefinitionInfo get_field_info(struct FlatbuffersParser* parser, int struct_index, int field_index);
void get_field_type_name(struct FlatbuffersParser* parser, int struct_index, int field_index, char* buf, int buf_len);

// Functions for included files
int get_num_included_files(struct FlatbuffersParser* parser);
void get_included_file_path(struct FlatbuffersParser* parser, int index, char* buf, int buf_len);

// Functions for documentation
void get_struct_documentation(struct FlatbuffersParser* parser, int index, char* buf, int buf_len);
void get_enum_documentation(struct FlatbuffersParser* parser, int index, char* buf, int buf_len);
void get_field_documentation(struct FlatbuffersParser* parser, int struct_index, int field_index, char* buf, int buf_len);
void get_enum_val_documentation(struct FlatbuffersParser* parser, int enum_index, int val_index, char* buf, int buf_len);


#ifdef __cplusplus
}
#endif
