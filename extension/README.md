# Helix Query Language Extension

This extension provides syntax highlighting and language support for Helix Query Language (HelixQL) files.

## Features

- Syntax highlighting for `.hx` and `.hql` files with automatic colorization
- Language server integration for diagnostics and hover information
- Support for HelixQL schema definitions (nodes, edges, vectors)
- Query syntax highlighting with proper scoping
- **Automatic color scheme** - works with any VS Code theme

## File Extensions

The extension supports files with the following extensions:
- `.hx` - Helix Query files
- `.hql` - Helix Query Language files

## Automatic Syntax Highlighting

The extension automatically provides rich syntax highlighting for HelixQL files with the following color-coded elements:

- **Keywords**: `QUERY`, `RETURN`, `WHERE`, etc. in purple/magenta
- **Schema Types**: `N::`, `E::`, `V::` in blue
- **Traversal Operators**: `::Out`, `::In`, `::WHERE` in yellow
- **Creation Operators**: `AddN`, `AddE`, `AddV` in teal
- **Punctuation**: Different colors for different bracket types:
  - `<>` (generics) in bright yellow
  - `()` (parameters) in magenta
  - `{}` (blocks) in gold
  - `[]` (arrays) in orange-red
- **Variables**: Light blue for identifiers and field names
- **Types**: Blue for primitive types like `String`, `I32`, etc.
- **Strings**: Orange for string literals
- **Numbers**: Light green for numeric values
- **Comments**: Green and italic

The colors are automatically applied to your existing theme - no configuration required!

## Advanced Customization (Optional)

If you want to customize the colors further, you can override them in your VS Code `settings.json`:

```json
{
  "editor.tokenColorCustomizations": {
    "textMateRules": [
      {
        "scope": "punctuation.definition.generic.begin.helixql",
        "settings": {
          "foreground": "#YOUR_COLOR_HERE"
        }
      }
    ]
  }
}
```

## Available Scopes

The following TextMate scopes are available for customization:

### Keywords
- `keyword.control.helixql` - Control keywords (RETURN, WHERE, etc.)
- `keyword.other.query.helixql` - QUERY keyword
- `keyword.other.node.helixql` - N:: prefix
- `keyword.other.edge.helixql` - E:: prefix  
- `keyword.other.vector.helixql` - V:: prefix
- `keyword.operator.traversal.helixql` - Traversal operators (::Out, ::In, etc.)
- `keyword.operator.creation.helixql` - Creation operators (AddN, AddE, etc.)

### Punctuation
- `punctuation.separator.double-colon.helixql` - :: separator
- `punctuation.separator.dot.helixql` - . separator
- `punctuation.separator.comma.helixql` - , separator
- `punctuation.definition.generic.begin.helixql` - < for generics
- `punctuation.definition.generic.end.helixql` - > for generics
- `punctuation.definition.parameters.begin.helixql` - ( for parameters
- `punctuation.definition.parameters.end.helixql` - ) for parameters
- `punctuation.definition.block.begin.helixql` - { for blocks
- `punctuation.definition.block.end.helixql` - } for blocks
- `punctuation.definition.array.begin.helixql` - [ for arrays
- `punctuation.definition.array.end.helixql` - ] for arrays

### Variables and Types
- `variable.other.helixql` - Variable names
- `variable.parameter.helixql` - Parameter names
- `variable.other.field.helixql` - Field names
- `entity.name.function.helixql` - Function names
- `entity.name.type.helixql` - Type names
- `support.type.primitive.helixql` - Primitive types

## Language Server

The extension includes a language server that provides:
- Real-time diagnostics for syntax and semantic errors
- Hover information for HelixQL keywords and operations
- Schema validation

## Requirements

- VS Code 1.63.0 or higher

## Installation

Install from the VS Code marketplace or install the `.vsix` file directly.

## Development

To build and package the extension:

```bash
npm install
npm run compile
npm run package
``` 