# -*- mode: shell-script -*-

depends org-ruby

##
## Convert README.org to README.md for crates.io
## (crates.io only renders Markdown)
##
if [ -f README.org ]; then
    org-ruby --translate markdown README.org > README.md.tmp
    if [ -f README.md ] && diff README.md README.md.tmp > /dev/null; then
        echo "No changes in README.md" >&2
        rm README.md.tmp
    else
        echo "Updating README.md" >&2
        mv README.md.tmp README.md
    fi
fi
