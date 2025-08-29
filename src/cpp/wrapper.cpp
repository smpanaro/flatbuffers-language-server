#include "wrapper.h"
#include "flatbuffers/idl.h"
#include <string>

// We use a C-style struct to hide the C++ Parser implementation from Rust.
struct FlatbuffersParser {
    flatbuffers::Parser impl;
};

// Helper function to recursively build a type name
std::string GetTypeName(const flatbuffers::Type& type) {
    switch (type.base_type) {
        case flatbuffers::BASE_TYPE_STRUCT: {
            if (type.struct_def) {
                return type.struct_def->name;
            }
            break;
        }
        case flatbuffers::BASE_TYPE_UNION: {
            if (type.enum_def) {
                return type.enum_def->name;
            }
            break;
        }
        case flatbuffers::BASE_TYPE_VECTOR: {
            // For vectors, we recursively call to get the element type name
            // and wrap it in brackets.
            return "[" + GetTypeName(type.VectorType()) + "]";
        }
        case flatbuffers::BASE_TYPE_UTYPE:
        case flatbuffers::BASE_TYPE_BOOL:
        case flatbuffers::BASE_TYPE_CHAR:
        case flatbuffers::BASE_TYPE_UCHAR:
        case flatbuffers::BASE_TYPE_SHORT:
        case flatbuffers::BASE_TYPE_USHORT:
        case flatbuffers::BASE_TYPE_INT:
        case flatbuffers::BASE_TYPE_UINT:
        case flatbuffers::BASE_TYPE_LONG:
        case flatbuffers::BASE_TYPE_ULONG: {
            if (type.enum_def) {
                return type.enum_def->name;
            }
            break;
        }
        default: {
            break;
        }
    }
    // Fallback for primitive types
    return flatbuffers::TypeName(type.base_type);
}

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
    info.line = 0;
    return info;
}

// Functions for fields
int get_num_fields(struct FlatbuffersParser* parser, int struct_index) {
    if (!parser || struct_index < 0 || static_cast<size_t>(struct_index) >= parser->impl.structs_.vec.size()) {
        return 0;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(struct_index)];
    return static_cast<int>(struct_def->fields.vec.size());
}

// A map to store dynamically created strings for type names.
// This is a workaround because we can't return std::string across FFI.
// We leak this memory, which is not ideal for a long-running server, but
// it's acceptable for now to get the logic working.
// A better solution would involve a more complex memory management strategy.
static std::map<const flatbuffers::FieldDef*, std::string> type_name_storage;

struct FieldDefinitionInfo get_field_info(struct FlatbuffersParser* parser, int struct_index, int field_index) {
    struct FieldDefinitionInfo info = { nullptr, nullptr };
    if (!parser || struct_index < 0 || static_cast<size_t>(struct_index) >= parser->impl.structs_.vec.size()) {
        return info;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(struct_index)];
    if (field_index < 0 || static_cast<size_t>(field_index) >= struct_def->fields.vec.size()) {
        return info;
    }
    auto field_def = struct_def->fields.vec[static_cast<size_t>(field_index)];

    // Filter out internal union _type fields.
    if (field_def->name.length() > 5 && field_def->name.substr(field_def->name.length() - 5) == "_type") {
        if (field_def->value.type.enum_def && field_def->value.type.enum_def->is_union) {
            return info; // It's an internal union type field, skip it.
        }
    }

    info.name = field_def->name.c_str();

    std::string type_name = GetTypeName(field_def->value.type);
    // Store the string in our map and return a pointer to it.
    type_name_storage[field_def] = type_name;
    info.type_name = type_name_storage[field_def].c_str();

    return info;
}
