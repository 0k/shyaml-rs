# -*- mode: shell-script -*-

##
## PACKAGE TIME SCRIPT - Generates README.md for crates.io
##
## This runs during `autogen.sh` before `cargo publish`. The generated
## README.md is what crates.io displays on the crate page.
##
## Note: build.rs also generates README.md (to OUT_DIR) for rustdoc,
## but that runs on the consumer's machine, not at publish time.
##

depends pandoc

## Lua filter: convert org-mode `:no_run yes` to markdown `rust,no_run`
## (rustdoc expects `rust,no_run` not pandoc's `{.rust .no_run}` format)
RUSTDOC_ATTRS_LUA='
function CodeBlock(el)
  if el.classes[1] == "rust" then
    local extra = {}
    for k, v in pairs(el.attributes) do
      local key = k:gsub("-", "_")
      if key == "no_run" or key == "norun" then
        table.insert(extra, "no_run")
      elseif key == "ignore" then
        table.insert(extra, "ignore")
      elseif key == "compile_fail" then
        table.insert(extra, "compile_fail")
      elseif key == "should_panic" then
        table.insert(extra, "should_panic")
      end
    end
    if #extra > 0 then
      local info = "rust," .. table.concat(extra, ",")
      return pandoc.RawBlock("markdown", "```" .. info .. "\n" .. el.text .. "```\n")
    end
  end
  return el
end
'

if [ -f README.org ]; then
    lua_filter=$(mktemp)
    printf '%s\n' "$RUSTDOC_ATTRS_LUA" > "$lua_filter"
    pandoc README.org -f org -t commonmark --lua-filter="$lua_filter" -o README.md.tmp || return 1
    rm -f "$lua_filter"

    # Append changelog: try gitchangelog (dynamic), fall back to CHANGELOG.md (static)
    if command -v gitchangelog >/dev/null 2>&1; then
        echo "" >> README.md.tmp
        echo "" >> README.md.tmp
        gitchangelog >> README.md.tmp
    elif [ -f CHANGELOG.md ]; then
        echo "" >> README.md.tmp
        echo "" >> README.md.tmp
        cat CHANGELOG.md >> README.md.tmp
    fi

    if [ -f README.md ] && diff README.md README.md.tmp > /dev/null; then
        echo "No changes in README.md" >&2
        rm README.md.tmp
    else
        echo "Updating README.md" >&2
        mv README.md.tmp README.md
    fi
fi
