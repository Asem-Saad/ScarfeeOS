# Runs prompts.jsonl against a local llama-server and writes results/<label>.jsonl.
# Tool calls on `command` prompts are checked automatically; everything else needs human grading.
#
#   .\run_eval.ps1 -Label qwen3-4b-q4 -ServerUrl http://127.0.0.1:8080
param(
    [Parameter(Mandatory = $true)][string]$Label,
    [string]$ServerUrl = 'http://127.0.0.1:8080',
    [int]$MaxTokens = 512,
    [string]$Model = ''
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$outDir = Join-Path $here 'results'
New-Item -ItemType Directory -Force $outDir | Out-Null
$outFile = Join-Path $outDir "$Label.jsonl"
if (Test-Path $outFile) { Remove-Item $outFile }
$utf8 = New-Object System.Text.UTF8Encoding($false)

function New-Tool($name, $description, $props, $required) {
    @{ type = 'function'; function = @{
        name = $name; description = $description
        parameters = @{ type = 'object'; properties = $props; required = $required }
    } }
}
function Str($d) { @{ type = 'string'; description = $d } }

$tools = @(
    (New-Tool 'open_app' 'Open an application.' @{ name = (Str 'App name') } @('name')),
    (New-Tool 'list_files' 'List files in a folder.' @{ path = (Str 'Folder path') } @('path')),
    (New-Tool 'search_files' 'Search for files by name.' @{ query = (Str 'Search text') } @('query')),
    (New-Tool 'create_reminder' 'Create a reminder.' @{ text = (Str 'Reminder text'); time = (Str 'ISO 8601 time') } @('text', 'time')),
    (New-Tool 'schedule_meeting' 'Schedule a meeting.' @{
        title = (Str 'Meeting title'); time = (Str 'ISO 8601 time')
        attendees = @{ type = 'array'; items = @{ type = 'string' } }
    } @('title', 'time', 'attendees')),
    (New-Tool 'set_language' 'Change the system language.' @{ lang = @{ type = 'string'; enum = @('en', 'ar') } } @('lang'))
)

$system = "You are the assistant built into ScarfeeOS. Today is $((Get-Date).ToString('dddd, yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture)). " +
    'Reply in the language the user wrote in. Call a tool only when one fits the request. Never invent data.'

$rows = Get-Content (Join-Path $here 'prompts.jsonl') -Encoding UTF8 | ForEach-Object { $_ | ConvertFrom-Json }
$checked = 0; $passed = 0; $failed = 0

foreach ($row in $rows) {
    $request = @{
        messages = @(
            @{ role = 'system'; content = $system },
            @{ role = 'user'; content = $row.prompt }
        )
        tools = $tools
        max_tokens = $MaxTokens
        temperature = 0
        chat_template_kwargs = @{ enable_thinking = $false }
    }
    if ($Model) { $request.model = $Model }
    $body = $request | ConvertTo-Json -Depth 12

    $started = Get-Date
    $reply = ''; $calls = @(); $err = $null
    try {
        $resp = Invoke-WebRequest -UseBasicParsing -Method Post -Uri "$ServerUrl/v1/chat/completions" `
            -ContentType 'application/json' -Body $utf8.GetBytes($body) -TimeoutSec 300
        $json = $utf8.GetString($resp.RawContentStream.ToArray()) | ConvertFrom-Json
        $message = $json.choices[0].message
        $reply = $message.content
        if ($message.tool_calls) { $calls = @($message.tool_calls | ForEach-Object { $_.function }) }
    } catch {
        $err = $_.Exception.Message
        $failed++
    }

    $toolOk = $null
    if ($row.tool -and -not $err) {
        $checked++
        $toolOk = [bool]($calls | Where-Object { $_.name -eq $row.tool.name })
        if ($toolOk) { $passed++ }
    }

    $result = [ordered]@{
        id = $row.id; category = $row.category; lang = $row.lang; prompt = $row.prompt; expect = $row.expect
        reply = $reply; tool_calls = $calls; tool_ok = $toolOk; error = $err
        seconds = [math]::Round(((Get-Date) - $started).TotalSeconds, 1)
    }
    [IO.File]::AppendAllText($outFile, (($result | ConvertTo-Json -Depth 8 -Compress) + "`n"), $utf8)
    Write-Output "$($row.id)  tool_ok=$toolOk  $($result.seconds)s"
}

Write-Output "Done: $($rows.Count) prompts, $failed errors, tool name matched $passed/$checked. Results: $outFile"
