_kanna() {
  local cur=${COMP_WORDS[COMP_CWORD]} line ext path
  local -a values=()
  COMPREPLY=()
  while IFS= read -r line; do
    case $line in
      :files) COMPREPLY+=($(compgen -f -- "$cur")) ;;
      ":files "*)
        while IFS= read -r path; do
          for ext in ${line#:files }; do
            [[ -d $path || $path == *."$ext" ]] && { COMPREPLY+=("$path"); break; }
          done
        done < <(compgen -f -- "$cur") ;;
      :dirs) COMPREPLY+=($(compgen -d -- "$cur")) ;;
      :commands) COMPREPLY+=($(compgen -c -- "$cur")) ;;
      ?*) values+=("${line%%$'\t'*}") ;;
    esac
  done < <("${COMP_WORDS[0]}" __complete -- "${COMP_WORDS[@]:1:COMP_CWORD}" 2>/dev/null)
  ((${#values[@]})) && COMPREPLY+=($(compgen -W "${values[*]}" -- "$cur"))
}

complete -o filenames -F _kanna kanna
