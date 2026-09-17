# -*- mode: shell-script -*-
##
## Vendored from 0k-pkg (``cargo/autogen.d/20-org-to-md.sh``).
## Refresh with ``pkg vendor``; to keep local edits, add a line
## ``## pkgcmd: local-override`` in the first five lines.
##
## Generates ``README.md`` from ``README.org`` for crates.io, and
## appends the changelog to it: cargo has no dedicated changelog
## field, the README is the only file crates.io renders.
##
## Tables are emitted as GFM pipe tables (``+pipe_tables``): plain
## ``commonmark`` falls back to raw HTML tables, which crates.io strips.
##
## Needs ``pandoc`` and ``gitchangelog``.  Without them this step is
## skipped with a warning (a fresh clone can still build); the release
## pipeline sets ``AUTOGEN_STRICT=1`` which makes them mandatory.
##

## Lua filter: convert org-mode ``:no_run yes`` (and friends) on a
## rust block to the ```` ```rust,no_run ```` fence rustdoc expects
## instead of pandoc's ``{.rust .no_run}`` attribute syntax.
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
    depends_soft pandoc || {
        echo "README.md not regenerated (fine for building, required for release)." >&2
        return 0
    }
    lua_filter=$(mktemp)
    printf '%s\n' "$RUSTDOC_ATTRS_LUA" > "$lua_filter"
    pandoc README.org -f org -t commonmark+pipe_tables --lua-filter="$lua_filter" -o README.md.tmp || {
        rm -f "$lua_filter" README.md.tmp
        return 1
    }
    rm -f "$lua_filter"

    if depends_soft gitchangelog; then
        if [ ! -e .gitchangelog.rc ]; then
            echo "No .gitchangelog.rc found: changelog not appended to README.md." >&2
        else
            echo "" >> README.md.tmp
            echo "" >> README.md.tmp
            gitchangelog >> README.md.tmp || {
                rm -f README.md.tmp
                return 1
            }
        fi
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
