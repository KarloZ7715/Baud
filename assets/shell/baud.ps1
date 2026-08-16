# Baud shell integration for PowerShell.
# Source from $PROFILE when BAUD_SHELL_INTEGRATION=1:
#   if ($env:BAUD_SHELL_INTEGRATION -eq '1' -and $env:BAUD_SHELL_INTEGRATION_SCRIPT) {
#       . $env:BAUD_SHELL_INTEGRATION_SCRIPT
#   }
if ($env:BAUD_SHELL_INTEGRATION_DONE) { return }
if ($env:TERM -ne 'xterm-256color') { return }
$env:BAUD_SHELL_INTEGRATION_DONE = '1'

function global:__BaudEmit([string]$s) {
    [Console]::Write($s)
}

$script:__baudOrigPrompt = $function:Prompt
function global:Prompt {
    $code = 0
    if ($null -ne $global:LASTEXITCODE) { $code = [int]$global:LASTEXITCODE }
    __BaudEmit ("`e]133;D;" + $code + "`a`e]133;A`a")
    $out = & $script:__baudOrigPrompt
    __BaudEmit "`e]133;B`a"
    $out
}

if (Get-Module -Name PSReadLine -ErrorAction SilentlyContinue) {
    Set-PSReadLineKeyHandler -Key Enter -ScriptBlock {
        [Console]::Write("`e]133;C`a")
        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
    }
}
