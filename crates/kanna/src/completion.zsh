#compdef kanna

_kanna() {
  local line ret=1
  local -a values files
  for line in "${(@f)$("${(Q)words[1]}" __complete -- "${(@Q)words[2,CURRENT]}" 2>/dev/null)}"; do
    case $line in
      (:files) _files && ret=0 ;;
      (:files\ *) files=(${=line#:files }); _files -g "*.(${(j:|:)files})" && ret=0 ;;
      (:dirs) _files -/ && ret=0 ;;
      (:commands) _command_names -e && ret=0 ;;
      (?*) values+=("${${line%%$'\t'*}//:/\\:}${${(M)line:#*$'\t'*}:+:${line#*$'\t'}}") ;;
    esac
  done
  (( $#values )) && _describe -V -t values kanna values && ret=0
  return ret
}

if [[ $zsh_eval_context[-1] == loadautofunc ]]; then
  _kanna "$@"
else
  (( $+functions[compdef] )) || { autoload -Uz compinit && compinit -i; }
  compdef _kanna kanna
fi
