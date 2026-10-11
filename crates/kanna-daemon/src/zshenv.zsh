if [[ -n "${KANNA_ZSH_ZDOTDIR+X}" ]]; then
  builtin export ZDOTDIR="$KANNA_ZSH_ZDOTDIR"
  builtin unset KANNA_ZSH_ZDOTDIR
else
  builtin unset ZDOTDIR
fi

{
  builtin typeset _kanna_file="${ZDOTDIR-$HOME}/.zshenv"
  [[ ! -r "$_kanna_file" ]] || builtin source -- "$_kanna_file"
} always {
  builtin unset _kanna_file
  if [[ -o interactive ]]; then
    _kanna_completion() {
      precmd_functions=(${precmd_functions:#_kanna_completion})
      builtin unfunction _kanna_completion
      (( $+commands[kanna] )) && builtin source <(command kanna completions zsh)
    }
    precmd_functions+=(_kanna_completion)
    _kanna_cwd() { [[ $PWD == *[[:cntrl:]]* ]] || builtin print -nr -- $'\e]7;kitty-shell-cwd://'"$PWD"$'\a' }
    _kanna_first_cwd() {
      precmd_functions=(${precmd_functions:#_kanna_first_cwd})
      builtin unfunction _kanna_first_cwd
      _kanna_cwd
    }
    chpwd_functions+=(_kanna_cwd)
    precmd_functions+=(_kanna_first_cwd)
  fi
}
