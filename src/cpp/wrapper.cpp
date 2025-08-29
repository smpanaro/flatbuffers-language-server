#include "wrapper.h"
#include "flatbuffers/idl.h"

// We use a C-style struct to hide the C++ Parser implementation from Rust.
struct FlatbuffersParser {
    flatbuffers::Parser impl;
};

struct FlatbuffersParser* parse_schema(const char* schema_content) {
    auto parser = new FlatbuffersParser();
    if (parser->impl.Parse(schema_content, nullptr, "")) {
        return parser;
    } else {
        return parser;
    }
}

void delete_parser(struct FlatbuffersParser* parser) {
    if (parser) {
        delete parser;
    }
}

const char* get_parser_error(struct FlatbuffersParser* parser) {
    if (!parser) {
        return "Invalid parser pointer.";
    }
    return parser->impl.error_.c_str();
}

bool is_parser_success(struct FlatbuffersParser* parser) {
    if (!parser) {
        return false;
    }
    return parser->impl.error_.empty();
}

// Functions for structs and tables
int get_num_structs(struct FlatbuffersParser* parser) {
    if (!parser) return 0;
    return static_cast<int>(parser->impl.structs_.vec.size());
}

struct StructDefinitionInfo get_struct_info(struct FlatbuffersParser* parser, int index) {
    struct StructDefinitionInfo info = { nullptr, false, 0 };
    if (!parser || index < 0 || static_cast<size_t>(index) >= parser->impl.structs_.vec.size()) {
        return info;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(index)];
    info.name = struct_def->name.c_str();
    info.is_table = !struct_def->fixed;
    // TODO: Find the correct way to get the line number.
    info.line = 0;
    return info;
}

// Functions for enums and unions
int get_num_enums(struct FlatbuffersParser* parser) {
    if (!parser) return 0;
    return static_cast<int>(parser->impl.enums_.vec.size());
}

struct EnumDefinitionInfo get_enum_info(struct FlatbuffersParser* parser, int index) {
    struct EnumDefinitionInfo info = { nullptr, false, 0 };
    if (!parser || index < 0 || static_cast<size_t>(index) >= parser->impl.enums_.vec.size()) {
        return info;
    }
    auto enum_def = parser->impl.enums_.vec[static_cast<size_t>(index)];
    info.name = enum_def->name.c_str();
    info.is_union = enum_def->is_union;
    // TODO: Find the correct way to get the line number.
    info.line = 0;
    return info;
}