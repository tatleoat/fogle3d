#!/bin/bash
# SPDX-License-Identifier: MIT
# Bundle check of the release model (unit pkg1); see check_bundle.py. Extra arguments are passed on.
cd "$(dirname "$0")/../../.." || exit 2
exec python3 -I re/tools/pkg/check_bundle.py "$@"
