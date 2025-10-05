#include "wrapper.h"
#include "flatbuffers/idl.h"
#include "flatbuffers/util.h"
#include <string>
#ifdef _WIN32
#include <direct.h>
#else
#include <unistd.h>
#endif
#include <vector>
#include <unordered_set>

// We use a C-style struct to hide the C++ Parser implementation from Rust.
struct FlatbuffersParser {
    flatbuffers::Parser impl;
    bool error;
    std::unordered_set<std::string> string_cache;
};

// Helper function to recursively build a type name
std::string GetTypeName(const flatbuffers::Type& type) {
    switch (type.base_type) {
        case flatbuffers::BASE_TYPE_STRUCT: {
            if (type.struct_def) {
                if (type.struct_def->defined_namespace) {
                    return type.struct_def->defined_namespace->GetFullyQualifiedName(type.struct_def->name);
                }
                return type.struct_def->name;
            }
            break;
        }
        case flatbuffers::BASE_TYPE_UNION: {
            if (type.enum_def) {
                if (type.enum_def->defined_namespace) {
                    return type.enum_def->defined_namespace->GetFullyQualifiedName(type.enum_def->name);
                }
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
                if (type.enum_def->defined_namespace) {
                    return type.enum_def->defined_namespace->GetFullyQualifiedName(type.enum_def->name);
                }
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

const char* join_doc_comments(const std::vector<std::string>& doc_comment, std::unordered_set<std::string>& string_cache) {
    if (doc_comment.empty()) return "";

    std::string full_doc;
    for (size_t i = 0; i < doc_comment.size(); ++i) {
        full_doc += doc_comment[i];
        if (i < doc_comment.size() - 1) {
            full_doc += "\n";
        }
    }

    if (!full_doc.empty()) {
        auto result = string_cache.insert(full_doc);
        return result.first->c_str();
    }
    return "";
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
#ifdef _WIN32
    if (_getcwd(cwd, sizeof(cwd)) != NULL) {
        include_paths.push_back(cwd);
    }
#else
    if (getcwd(cwd, sizeof(cwd)) != NULL) {
        include_paths.push_back(cwd);
    }
#endif
    include_paths.push_back(nullptr);

    const char** paths = include_paths.empty() ? nullptr : include_paths.data();

    parser->error = !parser->impl.Parse(schema_content, paths, filename ? filename : "");
    return parser;
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
    return !parser->error;
}

// Functions for structs and tables
int get_num_structs(struct FlatbuffersParser* parser) {
    if (!parser) return 0;
    return static_cast<int>(parser->impl.structs_.vec.size());
}

struct StructDefinitionInfo get_struct_info(struct FlatbuffersParser* parser, int index) {
    struct StructDefinitionInfo info = { nullptr, nullptr, nullptr, nullptr, false, 0, 0, 0, 0 };
    if (!parser || index < 0 || static_cast<size_t>(index) >= parser->impl.structs_.vec.size()) {
        return info;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(index)];
    info.name = struct_def->name.c_str();
    info.file = struct_def->file.c_str();

    if (struct_def->defined_namespace) {
        std::string ns;
        for (size_t i = 0; i < struct_def->defined_namespace->components.size(); ++i) {
            if (i > 0) {
                ns += ".";
            }
            ns += struct_def->defined_namespace->components[i];
        }
        auto result = parser->string_cache.insert(ns);
        info.namespace_ = result.first->c_str();
    }

    info.documentation = join_doc_comments(struct_def->doc_comment, parser->string_cache);
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
    struct EnumDefinitionInfo info = { nullptr, nullptr, nullptr, nullptr, false, 0, 0 };
    if (!parser || index < 0 || static_cast<size_t>(index) >= parser->impl.enums_.vec.size()) {
        return info;
    }
    auto enum_def = parser->impl.enums_.vec[static_cast<size_t>(index)];
    info.name = enum_def->name.c_str();
    info.file = enum_def->file.c_str();

    if (enum_def->defined_namespace) {
        std::string ns;
        for (size_t i = 0; i < enum_def->defined_namespace->components.size(); ++i) {
            if (i > 0) {
                ns += ".";
            }
            ns += enum_def->defined_namespace->components[i];
        }
        auto result = parser->string_cache.insert(ns);
        info.namespace_ = result.first->c_str();
    }

    info.documentation = join_doc_comments(enum_def->doc_comment, parser->string_cache);
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
    struct EnumValDefinitionInfo info = { nullptr, nullptr, 0, 0, 0, {}, {}};
    if (!parser || enum_index < 0 || static_cast<size_t>(enum_index) >= parser->impl.enums_.vec.size()) {
        return info;
    }
    auto enum_def = parser->impl.enums_.vec[static_cast<size_t>(enum_index)];
    if (val_index < 0 || static_cast<size_t>(val_index) >= enum_def->Vals().size()) {
        return info;
    }
    auto enum_val = enum_def->Vals()[static_cast<size_t>(val_index)];
    info.name = enum_val->name.c_str();
    if (enum_def->is_union) {
        std::string name = GetTypeName(enum_val->union_type);
        auto result = parser->string_cache.insert(name);
        info.name = result.first->c_str(); // fully-qualified name
    }

    info.documentation = join_doc_comments(enum_val->doc_comment, parser->string_cache);
    info.value = enum_val->GetAsInt64();
    info.line = enum_val->decl_line - 1;
    info.col = enum_val->decl_col;

    auto def_range = enum_val->decl_range;
    info.type_range.start.line = def_range.start.line - 1; // parser line is 1-based
    info.type_range.start.col = def_range.start.col;
    info.type_range.end.line = def_range.end.line - 1;
    info.type_range.end.col = def_range.end.col;

    info.type_source = enum_val->decl_text.c_str();

    return info;
}

bool has_root_type(struct FlatbuffersParser* parser) {
    if (!parser) return false;
    return parser->impl.root_struct_def_ != nullptr && parser->impl.root_type_loc_ != nullptr;;
}

struct RootTypeDefinitionInfo get_root_type_info(struct FlatbuffersParser* parser) {
    struct RootTypeDefinitionInfo info = { nullptr, nullptr, {}, nullptr};
    if (!parser || !has_root_type(parser)) return info;
    auto root_def = parser->impl.root_struct_def_;

    info.name = root_def->name.c_str();
    if (root_def->defined_namespace) {
        std::string fqn = root_def->defined_namespace->GetFullyQualifiedName(root_def->name);
        auto result = parser->string_cache.insert(fqn);
        info.name = result.first->c_str();
    }
    info.file = parser->impl.root_type_loc_->filename_.c_str();

    auto def_range = parser->impl.root_type_loc_->decl_range;
    info.type_range.start.line = def_range.start.line - 1; // parser line is 1-based
    info.type_range.start.col = def_range.start.col;
    info.type_range.end.line = def_range.end.line - 1;
    info.type_range.end.col = def_range.end.col;

    info.type_source = parser->impl.root_type_loc_->decl_text.c_str();

    return info;
}

int get_num_all_included_files(struct FlatbuffersParser* parser) {
    if (!parser) return 0;
    int count = 0;
    for (const auto& pair : parser->impl.files_included_per_file_) {
        count += pair.second.size();
    }
    return count;
}

const char* get_all_included_file_path(struct FlatbuffersParser* parser, int index) {

    if (!parser) return "";

    int current_index = 0;
    for (const auto& pair : parser->impl.files_included_per_file_) {
        for (const auto& included_file : pair.second) {
            if (current_index == index) {
                auto result = parser->string_cache.insert(included_file.filename);
                return result.first->c_str();
            }
            current_index++;
        }
    }
    return "";
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
    struct FieldDefinitionInfo info = { nullptr, nullptr, nullptr, nullptr, 0, 0, {}, nullptr, false, false, 0 };
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

    {
        std::string type_name = GetTypeName(field_def->value.type);
        auto result = parser->string_cache.insert(type_name);
        info.type_name = result.first->c_str();
    }

    {
        flatbuffers::Type type  = field_def->value.type;
        switch (type.base_type) {
            case flatbuffers::BASE_TYPE_VECTOR:
            case flatbuffers::BASE_TYPE_ARRAY: {
                type = type.VectorType();
            }
            default:
                break;
        }
        std::string type_name = GetTypeName(type);
        auto result = parser->string_cache.insert(type_name);
        info.base_type_name = result.first->c_str();
    }

    info.documentation = join_doc_comments(field_def->doc_comment, parser->string_cache);

    info.line = field_def->decl_line - 1;
    info.col = field_def->decl_col;
    info.deprecated = field_def->deprecated;

    auto def_range = field_def->value.type.decl_range;
    info.type_range.start.line = def_range.start.line - 1; // parser line is 1-based
    info.type_range.start.col = def_range.start.col;
    info.type_range.end.line = def_range.end.line - 1;
    info.type_range.end.col = def_range.end.col;

    info.type_source = field_def->value.type.decl_text.c_str();

    auto id_attr = field_def->attributes.Lookup("id");
    if (id_attr) {
        info.has_id = true;
        info.id = std::stoi(id_attr->constant);
    }

    return info;
}

flatbuffers::FieldDef* get_field_def(struct FlatbuffersParser* parser, int struct_index, int field_index) {
    if (!parser) return nullptr;

    if (struct_index < 0 || static_cast<size_t>(struct_index) >= parser->impl.structs_.vec.size()) {
        return nullptr;
    }
    auto struct_def = parser->impl.structs_.vec[static_cast<size_t>(struct_index)];
    if (field_index < 0 || static_cast<size_t>(field_index) >= struct_def->fields.vec.size()) {
        return nullptr;
    }
    return struct_def->fields.vec[static_cast<size_t>(field_index)];
}

// Functions for include graph
int get_num_files_with_includes(struct FlatbuffersParser* parser) {
    if (!parser) return 0;
    return static_cast<int>(parser->impl.files_included_per_file_.size());
}

const char* get_file_with_includes_path(struct FlatbuffersParser* parser, int index) {
    if (!parser) return "";

    int current_index = 0;
    for (const auto& pair : parser->impl.files_included_per_file_) {
        if (current_index == index) {
            auto result = parser->string_cache.insert(pair.first);
            return result.first->c_str();
        }
        current_index++;
    }
    return "";
}

int get_num_includes_for_file(struct FlatbuffersParser* parser, const char* file_path) {
    if (!parser || !file_path) return 0;
    auto it = parser->impl.files_included_per_file_.find(file_path);
    if (it != parser->impl.files_included_per_file_.end()) {
        return static_cast<int>(it->second.size());
    }
    return 0;
}

const char* get_included_file_path(struct FlatbuffersParser* parser, const char* file_path, int index) {
    if (!parser || !file_path) return "";

    auto it = parser->impl.files_included_per_file_.find(file_path);
    if (it != parser->impl.files_included_per_file_.end()) {
        if (index >= 0 && static_cast<size_t>(index) < it->second.size()) {
            auto set_it = it->second.begin();
            std::advance(set_it, index);
            auto result = parser->string_cache.insert(set_it->filename);
            return result.first->c_str();
        }
    }
    return "";
}
