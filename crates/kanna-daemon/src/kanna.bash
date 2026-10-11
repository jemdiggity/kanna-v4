_kanna_hook='. "$KANNA_BASH_INIT"'
PROMPT_COMMAND=${PROMPT_COMMAND//"$_kanna_hook"/}
PROMPT_COMMAND=${PROMPT_COMMAND//'; ;'/;}
while [[ $PROMPT_COMMAND == *[\;\ ] ]]; do PROMPT_COMMAND=${PROMPT_COMMAND%?}; done
while [[ $PROMPT_COMMAND == [\;\ ]* ]]; do PROMPT_COMMAND=${PROMPT_COMMAND#?}; done
export -n PROMPT_COMMAND
_kanna_cwd() { [[ $PWD == "${_kanna_pwd-}" ]] || { _kanna_pwd=$PWD; printf '\e]7;kitty-shell-cwd://%s\a' "$PWD"; }; }
PROMPT_COMMAND="_kanna_cwd${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
_kanna_cwd
unset KANNA_BASH_INIT _kanna_hook
if type -P kanna >/dev/null; then eval "$(command kanna completions bash)"; fi
