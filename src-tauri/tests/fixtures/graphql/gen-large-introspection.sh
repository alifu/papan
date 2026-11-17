#!/bin/sh
# Prints a JSON document of about 23 MiB to stdout, larger than the 20 MiB introspection cap.
# Usage: sh gen-large-introspection.sh > /some/scratch/dir/big.json   (do not commit the output)
set -eu
printf '{"data":{"__schema":{"description":"'
head -c 24117248 /dev/zero | tr '\0' 'a'
printf '"}}}\n'
