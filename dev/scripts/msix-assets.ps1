# PowerShell 7 / Windows。パッケージ用画像をコードから生成する。
function New-MsixAssets {
    param([Parameter(Mandatory)][string]$Directory)
    Add-Type -AssemblyName System.Drawing
    [IO.Directory]::CreateDirectory($Directory) | Out-Null
    foreach ($item in @(@('StoreLogo.png',50), @('Square44x44Logo.png',44), @('Square150x150Logo.png',150))) {
        $bitmap = [Drawing.Bitmap]::new([int]$item[1], [int]$item[1])
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $pen = [Drawing.Pen]::new([Drawing.ColorTranslator]::FromHtml('#2563EB'), 7)
        $brush = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml('#2563EB'))
        try {
            $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
            $graphics.Clear([Drawing.ColorTranslator]::FromHtml('#F3F6FA'))
            $graphics.ScaleTransform($item[1] / 100.0, $item[1] / 100.0)
            $pen.StartCap = $pen.EndCap = [Drawing.Drawing2D.LineCap]::Round
            $graphics.DrawLine($pen, 22, 50, 43, 50)
            $graphics.DrawLine($pen, 43, 50, 68, 25)
            $graphics.DrawLine($pen, 43, 50, 68, 75)
            $graphics.FillEllipse($brush, 13, 41, 18, 18)
            $graphics.FillEllipse($brush, 61, 16, 18, 18)
            $graphics.FillEllipse($brush, 61, 66, 18, 18)
            $bitmap.Save((Join-Path $Directory $item[0]), [Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $brush.Dispose(); $pen.Dispose(); $graphics.Dispose(); $bitmap.Dispose()
        }
    }
}
