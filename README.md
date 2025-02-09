# SAPF Language Server

An lsp-server for [sapf](https://github.com/lfnoise/sapf).
Has some syntax highlighting, autocompletion and hover support for docs. Might work poorly.

```rust
cargo build --release
```

```lua
local lspconfig = require("lspconfig")
local configs = require("lspconfig.configs")

if not configs.sapf then
  configs.sapf = {
    default_config = {
        cmd = { "sapf-lsp" }, -- path to the executable
      root_dir = lspconfig.util.root_pattern("*.sapf"),
      filetypes = { "sapf" },
    },
  }
end
lspconfig.sapf.setup({})
```
