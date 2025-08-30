#pragma once

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Opaque pointer to the flatbuffers::Parser
struct FlatbuffersParser;

// A struct to pass struct/table definition information
struct StructDefinitionInfo {
    const char* name;
    bool is_table;
    int line; // Note: This is not yet implemented correctly
};

// A struct to pass enum/union definition information
struct EnumDefinitionInfo {
    const char* name;
    bool is_union;
    int line; // Note: This is not yet implemented correctly
};

// A struct to pass field information
struct FieldDefinitionInfo {
    const char* name;
};

// Parses a schema and returns a pointer to the Parser object.
struct FlatbuffersParser* parse_schema(const char* schema_content);

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

// Functions for fields
int get_num_fields(struct FlatbuffersParser* parser, int struct_index);
struct FieldDefinitionInfo get_field_info(struct FlatbuffersParser* parser, int struct_index, int field_index);
void get_field_type_name(struct FlatbuffersParser* parser, int struct_index, int field_index, char* buf, int buf_len);


#ifdef __cplusplus
}
#endif
