#include "wrapper.h"
#include "flatbuffers/idl.h"
#include "flatbuffers/util.h"
#include <string>
#include <unistd.h>
#include <vector>

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
            return "[" + GetTypeName(type.VectorType()) + "]";
        }
        case flatbuffers::BASE_TYPE_ARRAY: {
            return "[" + GetTypeName(type.VectorType()) + ":" + std::to_string(type.fixed_length) + "]";
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
    return flatbuffers::TypeName(type.base_type);
}

struct FlatbuffersParser* parse_schema(const char* schema_content, const char* filename) {
    auto parser = new FlatbuffersParser();

    std::vector<const char*> include_paths;
    std::string path_str;
    if (filename && strlen(filename) > 0) {
        path_str = flatbuffers::StripFileName(filename);
        include_paths.push_back(path_str.c_str());
    }
    // Add CWD to include paths
    char cwd[1024];
    if (getcwd(cwd, sizeof(cwd)) != NULL) {
        include_paths.push_back(cwd);
    }
    include_paths.push_back(nullptr);

    const char** paths = include_paths.empty() ? nullptr : include_paths.data();

    if (parser->impl.Parse(schema_content, paths, filename ? filename : "")) {
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
    struct StructDefinitionInfo info = { nullptr, nullptr, false, 0, 0, 0, 0 };
    if (!parser || index < 0 || static_cast<size_t>(index) >= parser->impl.structs_.vec.size()) {
        return info;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(index)];
    info.name = struct_def->name.c_str();
    info.file = struct_def->file.c_str();
    info.is_table = !struct_def->fixed;
    info.line = struct_def->decl_line - 1;
    info.col = struct_def->decl_col;
    info.bytesize = struct_def->bytesize;
    info.minalign = struct_def->minalign;
    return info;
}

// Functions for enums and unions
int get_num_enums(struct FlatbuffersParser* parser) {
    if (!parser) return 0;
    return static_cast<int>(parser->impl.enums_.vec.size());
}

struct EnumDefinitionInfo get_enum_info(struct FlatbuffersParser* parser, int index) {
    struct EnumDefinitionInfo info = { nullptr, nullptr, false, 0, 0 };
    if (!parser || index < 0 || static_cast<size_t>(index) >= parser->impl.enums_.vec.size()) {
        return info;
    }
    auto enum_def = parser->impl.enums_.vec[static_cast<size_t>(index)];
    info.name = enum_def->name.c_str();
    info.file = enum_def->file.c_str();
    info.is_union = enum_def->is_union;
    info.line = enum_def->decl_line - 1;
    info.col = enum_def->decl_col;
    return info;
}

int get_num_enum_vals(struct FlatbuffersParser* parser, int enum_index) {
    if (!parser || enum_index < 0 || static_cast<size_t>(enum_index) >= parser->impl.enums_.vec.size()) {
        return 0;
    }
    auto enum_def = parser->impl.enums_.vec[static_cast<size_t>(enum_index)];
    return static_cast<int>(enum_def->Vals().size());
}

struct EnumValDefinitionInfo get_enum_val_info(struct FlatbuffersParser* parser, int enum_index, int val_index) {
    struct EnumValDefinitionInfo info = { nullptr, 0, 0, 0 };
    if (!parser || enum_index < 0 || static_cast<size_t>(enum_index) >= parser->impl.enums_.vec.size()) {
        return info;
    }
    auto enum_def = parser->impl.enums_.vec[static_cast<size_t>(enum_index)];
    if (val_index < 0 || static_cast<size_t>(val_index) >= enum_def->Vals().size()) {
        return info;
    }
    auto enum_val = enum_def->Vals()[static_cast<size_t>(val_index)];
    info.name = enum_val->name.c_str();
    info.value = enum_val->GetAsInt64();
    info.line = enum_val->decl_line - 1;
    info.col = enum_val->decl_col;
    return info;
}

bool has_root_type(struct FlatbuffersParser* parser) {
    if (!parser) return false;
    return parser->impl.root_struct_def_ != nullptr && parser->impl.root_type_loc_ != nullptr;;
}

struct RootTypeDefinitionInfo get_root_type_info(struct FlatbuffersParser* parser) {
    struct RootTypeDefinitionInfo info = { nullptr, nullptr, 0, 0};
    if (!parser || !has_root_type(parser)) return info;
    auto root_def = parser->impl.root_struct_def_;
    info.name = root_def->name.c_str();
    info.file = parser->impl.root_type_loc_->filename_.c_str();
    info.line = parser->impl.root_type_loc_->line_ - 1;
    info.col = parser->impl.root_type_loc_->col_;
    return info;
}

void join_doc_comments(const std::vector<std::string>& doc_comment, char* buf, int buf_len) {
    if (buf == nullptr || buf_len <= 0) return;
    buf[0] = '\0';

    std::string full_doc;
    for (size_t i = 0; i < doc_comment.size(); ++i) {
        full_doc += doc_comment[i];
        if (i < doc_comment.size() - 1) {
            full_doc += "\n";
        }
    }

    if (!full_doc.empty()) {
        strncpy(buf, full_doc.c_str(), buf_len - 1);
        buf[buf_len - 1] = '\0';
    }
}

int get_num_all_included_files(struct FlatbuffersParser* parser) {
    if (!parser) return 0;
    int count = 0;
    for (const auto& pair : parser->impl.files_included_per_file_) {
        count += pair.second.size();
    }
    return count;
}

void get_all_included_file_path(struct FlatbuffersParser* parser, int index, char* buf, int buf_len) {

    if (!parser || buf == nullptr || buf_len <= 0) {
        if (buf) buf[0] = '\0';
        return;
    }
    buf[0] = '\0';

    int current_index = 0;
    for (const auto& pair : parser->impl.files_included_per_file_) {
        for (const auto& included_file : pair.second) {
            if (current_index == index) {
                strncpy(buf, included_file.filename.c_str(), buf_len - 1);
                buf[buf_len - 1] = '\0'; // Ensure null termination
                return;
            }
            current_index++;
        }
    }
}

// Functions for fields
int get_num_fields(struct FlatbuffersParser* parser, int struct_index) {
    if (!parser || struct_index < 0 || static_cast<size_t>(struct_index) >= parser->impl.structs_.vec.size()) {
        return 0;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(struct_index)];
    return static_cast<int>(struct_def->fields.vec.size());
}

struct FieldDefinitionInfo get_field_info(struct FlatbuffersParser* parser, int struct_index, int field_index) {
    struct FieldDefinitionInfo info = { nullptr, 0, 0, 0, 0, false };
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
    info.line = field_def->decl_line - 1;
    info.col = field_def->decl_col;
    info.type_line = field_def->type_decl_line - 1;
    info.deprecated = field_def->deprecated;

    // For vectors and arrays, type_decl_col points to start of type name,
    // but we want it to point to the end like everything else
    if (field_def->value.type.base_type == flatbuffers::BASE_TYPE_VECTOR ||
        field_def->value.type.base_type == flatbuffers::BASE_TYPE_ARRAY) {
        std::string type_name = GetTypeName(field_def->value.type);
        info.type_col = field_def->type_decl_col + static_cast<unsigned>(type_name.length()) - 1;
    } else {
        info.type_col = field_def->type_decl_col;
    }

    return info;
}

void get_field_type_name(struct FlatbuffersParser* parser, int struct_index, int field_index, char* buf, int buf_len) {
    if (!parser || buf == nullptr || buf_len <= 0) return;

    if (struct_index < 0 || static_cast<size_t>(struct_index) >= parser->impl.structs_.vec.size()) {
        buf[0] = '\0';
        return;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(struct_index)];
    if (field_index < 0 || static_cast<size_t>(field_index) >= struct_def->fields.vec.size()) {
        buf[0] = '\0';
        return;
    }
    auto field_def = struct_def->fields.vec[static_cast<size_t>(field_index)];

    std::string type_name = GetTypeName(field_def->value.type);
    strncpy(buf, type_name.c_str(), buf_len - 1);
    buf[buf_len - 1] = '\0'; // Ensure null termination
}

// Functions for documentation
void get_struct_documentation(struct FlatbuffersParser* parser, int index, char* buf, int buf_len) {
    if (!parser || index < 0 || static_cast<size_t>(index) >= parser->impl.structs_.vec.size()) {
        if (buf && buf_len > 0) buf[0] = '\0';
        return;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(index)];
    join_doc_comments(struct_def->doc_comment, buf, buf_len);
}

void get_enum_documentation(struct FlatbuffersParser* parser, int index, char* buf, int buf_len) {
    if (!parser || index < 0 || static_cast<size_t>(index) >= parser->impl.enums_.vec.size()) {
        if (buf && buf_len > 0) buf[0] = '\0';
        return;
    }
    auto enum_def = parser->impl.enums_.vec[static_cast<size_t>(index)];
    join_doc_comments(enum_def->doc_comment, buf, buf_len);
}

void get_field_documentation(struct FlatbuffersParser* parser, int struct_index, int field_index, char* buf, int buf_len) {
    if (!parser || struct_index < 0 || static_cast<size_t>(struct_index) >= parser->impl.structs_.vec.size()) {
        if (buf && buf_len > 0) buf[0] = '\0';
        return;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(struct_index)];
    if (field_index < 0 || static_cast<size_t>(field_index) >= struct_def->fields.vec.size()) {
        if (buf && buf_len > 0) buf[0] = '\0';
        return;
    }
    auto field_def = struct_def->fields.vec[static_cast<size_t>(field_index)];
    join_doc_comments(field_def->doc_comment, buf, buf_len);
}

void get_enum_val_documentation(struct FlatbuffersParser* parser, int enum_index, int val_index, char* buf, int buf_len) {
    if (!parser || enum_index < 0 || static_cast<size_t>(enum_index) >= parser->impl.enums_.vec.size()) {
        if (buf && buf_len > 0) buf[0] = '\0';
        return;
    }
    auto enum_def = parser->impl.enums_.vec[static_cast<size_t>(enum_index)];
    if (val_index < 0 || static_cast<size_t>(val_index) >= enum_def->Vals().size()) {
        if (buf && buf_len > 0) buf[0] = '\0';
        return;
    }
    auto enum_val = enum_def->Vals()[static_cast<size_t>(val_index)];
    join_doc_comments(enum_val->doc_comment, buf, buf_len);
}
