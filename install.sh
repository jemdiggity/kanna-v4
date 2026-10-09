#!/usr/bin/env bash
# Installs these skills into a project (or your home directory) for both
# Codex and Claude Code:
#   <dir>/.agents/skills/<name>   the skill (Codex reads it here)
#   <dir>/.claude/skills/<name>   a symlink to it (Claude Code reads it here)
#
# usage: install.sh [--global] [--link] [--force] [DIR] [SKILL...]
#   DIR       the project to install into (default: the current directory)
#   SKILL     only these skills (default: all of them)
#   --global  install into your home directory, for every project
#   --link    link to this repo instead of copying, so `git pull` here updates them
#   --force   replace skills that are already installed
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
dir="$PWD" link=0 force=0 skills=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --global) dir="$HOME" ;;
    --link) link=1 ;;
    --force) force=1 ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "install.sh: unknown option $1 (see --help)" >&2; exit 2 ;;
    *) if [[ -d "$here/skills/$1" ]]; then skills+=("$1"); elif [[ -d "$1" ]]; then dir="$(cd "$1" && pwd)"; else
         echo "install.sh: \"$1\" is neither a skill ($(ls "$here/skills" | paste -sd, - | sed 's/,/, /g')) nor a directory" >&2; exit 2; fi ;;
  esac
  shift
done
[[ ${#skills[@]} -gt 0 ]] || skills=($(ls "$here/skills"))

mkdir -p "$dir/.agents/skills" "$dir/.claude/skills"
for name in "${skills[@]}"; do
  target="$dir/.agents/skills/$name"
  if [[ -e "$target" || -L "$target" ]]; then
    if [[ $force -eq 0 ]]; then echo "skip $name: already in $dir/.agents/skills (--force to replace)"; continue; fi
    rm -rf "$target"
  fi
  if [[ $link -eq 1 ]]; then ln -s "$here/skills/$name" "$target"; else cp -R "$here/skills/$name" "$target"; fi
  claude="$dir/.claude/skills/$name"
  if [[ -e "$claude" || -L "$claude" ]]; then rm -rf "$claude"; fi
  ln -s "../../.agents/skills/$name" "$claude"
  echo "installed $name → $target$([[ $link -eq 1 ]] && echo " (linked)")"
done
