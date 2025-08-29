#pragma once

#ifdef __cplusplus
extern "C" {
#endif

// Opaque pointer to the flatbuffers::Parser
struct FlatbuffersParser;

// Parses a schema and returns a pointer to the Parser object.
// The caller is responsible for deleting the parser using `delete_parser`.
struct FlatbuffersParser* parse_schema(const char* schema_content);

// Deletes a parser object.
void delete_parser(struct FlatbuffersParser* parser);

// Returns the error string from the parser. The caller must not free this string.
const char* get_parser_error(struct FlatbuffersParser* parser);


#ifdef __cplusplus
}
#endif
