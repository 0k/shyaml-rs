# -*- mode: shell-script -*-
##
## Vendored from 0k-pkg (``autogen/autogen.d/90-prepare.sh``).
## Refresh with ``pkg vendor``; to keep local edits, add a line
## ``## pkgcmd: local-override`` in the first five lines.
##
## Substitutes ``%%version%%``-style placeholders in the files listed
## by ``$FILES`` in ``.package.d/config``.
##

prepare_files || print_error "Error while updating version information."
