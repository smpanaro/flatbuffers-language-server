#include "wrapper.h"
#include "flatbuffers/idl.h"

// We use a C-style struct to hide the C++ Parser implementation from Rust.
struct FlatbuffersParser {
    flatbuffers::Parser impl;
};

struct FlatbuffersParser* parse_schema(const char* schema_content) {
    auto parser = new FlatbuffersParser();
    // The flatbuffers parser requires a file path for include resolution,
    // but we are parsing from memory. We can provide a dummy path.
    // The second argument is the list of include paths, which is null for now.
    if (parser->impl.Parse(schema_content, nullptr, "")) {
        // On success, return the parser
        return parser;
    } else {
        // On failure, we still return the parser so the caller can get the error.
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
