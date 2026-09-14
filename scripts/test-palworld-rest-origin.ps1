[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ProfilePath,

    [string]$OutputPath,

    [ValidateRange(1, 30)]
    [int]$TimeoutSeconds = 5,

    [switch]$AllowInsecureDiscovery
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Net.Http

function Read-RestOriginProfile {
    param([Parameter(Mandatory = $true)][string]$Path)

    $resolved = (Resolve-Path -LiteralPath $Path).Path
    $restProfile = Get-Content -LiteralPath $resolved -Raw | ConvertFrom-Json
    $allowedProperties = @(
        'schemaVersion',
        'worldAlias',
        'gameEndpoint',
        'queryEndpoint',
        'rconEndpoint',
        'restBaseUrl'
    )
    $actualProperties = @($restProfile.PSObject.Properties.Name)
    $unknown = @($actualProperties | Where-Object { $_ -notin $allowedProperties })
    $missing = @($allowedProperties | Where-Object { $_ -notin $actualProperties })

    if ($unknown.Count -ne 0 -or $missing.Count -ne 0) {
        throw 'REST origin profile has unknown or missing fields.'
    }
    if ($restProfile.schemaVersion -ne 1) {
        throw 'REST origin profile schemaVersion must be 1.'
    }
    if ([string]::IsNullOrWhiteSpace($restProfile.worldAlias)) {
        throw 'REST origin profile worldAlias must not be empty.'
    }

    $uri = [Uri]$restProfile.restBaseUrl
    if (-not $uri.IsAbsoluteUri) {
        throw 'REST base URL must be absolute.'
    }
    if ($uri.Scheme -notin @('http', 'https')) {
        throw 'REST base URL must use HTTP or HTTPS.'
    }
    if ($uri.AbsolutePath.TrimEnd('/') -ne '/v1/api') {
        throw 'REST base URL path must be exactly /v1/api.'
    }
    if (-not [string]::IsNullOrEmpty($uri.UserInfo) -or
        -not [string]::IsNullOrEmpty($uri.Query) -or
        -not [string]::IsNullOrEmpty($uri.Fragment)) {
        throw 'REST base URL must not contain credentials, query, or fragment.'
    }

    [pscustomobject]@{
        Profile = $restProfile
        Uri = $uri
    }
}

function Invoke-UnauthenticatedRestProbe {
    param(
        [Parameter(Mandatory = $true)][Uri]$BaseUri,
        [Parameter(Mandatory = $true)][int]$Timeout
    )

    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.AllowAutoRedirect = $false
    $handler.UseProxy = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds($Timeout)
    $client.DefaultRequestHeaders.UserAgent.ParseAdd('PalCompanion-Rest-Origin-Probe/1')

    try {
        $results = foreach ($name in @('info', 'game-data', 'metrics', 'settings')) {
            $requestUri = [Uri]"$($BaseUri.AbsoluteUri.TrimEnd('/'))/$name"
            try {
                $response = $client.GetAsync($requestUri).GetAwaiter().GetResult()
                try {
                    $challenge = [string]$response.Headers.WwwAuthenticate
                    [pscustomobject]@{
                        endpoint = "/$name"
                        reachable = $true
                        statusCode = [int]$response.StatusCode
                        basicChallenge = $challenge -match '(?i)\bBasic\b'
                        palRealm = $challenge -match '(?i)realm="?Pal"?'
                        redirected = [int]$response.StatusCode -ge 300 -and
                            [int]$response.StatusCode -lt 400
                        error = $null
                    }
                }
                finally {
                    $response.Dispose()
                }
            }
            catch {
                [pscustomobject]@{
                    endpoint = "/$name"
                    reachable = $false
                    statusCode = $null
                    basicChallenge = $false
                    palRealm = $false
                    redirected = $false
                    error = $_.Exception.GetType().Name
                }
            }
        }
        return @($results)
    }
    finally {
        $client.Dispose()
        $handler.Dispose()
    }
}

$loaded = Read-RestOriginProfile -Path $ProfilePath
$endpointResults = Invoke-UnauthenticatedRestProbe `
    -BaseUri $loaded.Uri `
    -Timeout $TimeoutSeconds

$officialApiDetected = @(
    $endpointResults | Where-Object {
        $_.reachable -and
        $_.statusCode -eq 401 -and
        $_.basicChallenge -and
        $_.palRealm -and
        -not $_.redirected
    }
).Count -eq 4
$tlsProtected = $loaded.Uri.Scheme -eq 'https'
$productionUsable = $officialApiDetected -and $tlsProtected

$status = if (-not $officialApiDetected) {
    'not_confirmed'
}
elseif (-not $tlsProtected) {
    'reachable_but_insecure'
}
else {
    'production_candidate'
}

$report = [ordered]@{
    schemaVersion = 1
    worldAlias = [string]$loaded.Profile.worldAlias
    gameEndpoint = [string]$loaded.Profile.gameEndpoint
    restBaseUrl = $loaded.Uri.AbsoluteUri.TrimEnd('/')
    testedAtUtc = [DateTime]::UtcNow.ToString('o')
    officialApiDetected = $officialApiDetected
    tlsProtected = $tlsProtected
    credentialsSent = $false
    productionUsable = $productionUsable
    status = $status
    endpoints = $endpointResults
}

$encoded = $report | ConvertTo-Json -Depth 5
if (-not [string]::IsNullOrWhiteSpace($OutputPath)) {
    $parent = Split-Path -Parent $OutputPath
    if (-not [string]::IsNullOrWhiteSpace($parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    Set-Content -LiteralPath $OutputPath -Value $encoded -Encoding UTF8
}
$encoded

if (-not $officialApiDetected) {
    exit 1
}
if (-not $tlsProtected -and -not $AllowInsecureDiscovery) {
    exit 2
}
exit 0
