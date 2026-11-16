Attribute VB_Name = "modTreasury"
Option Explicit

' 자금 지급일정 대시보드 매크로
'  - Setup        : 처음 한 번 실행. 환율 버튼을 만들고 차트 더블클릭을 연결합니다.
'  - Auto_Open    : 파일을 열 때마다 차트 더블클릭을 다시 연결합니다.
'  - FetchFxRates : 서울외국환중개(www.smbs.biz)에서 기준일 매매기준율을 가져옵니다.

Private Const SH_DASH As String = "대시보드"
Private Const SH_DETAIL As String = "상세"
Private Const SH_FX As String = "환율·금리"
Private Const FIRST_MONTH_ROW As Long = 32       ' 대시보드 월별 표 첫 행 (B열 = 월말일)
Private Const SMBS_URL As String = "http://www.smbs.biz/ExRate/StdExRate_xml.jsp?arr_value="

Private gChart As clsChartEvents

Public Sub Auto_Open()
    HookChart
End Sub

Public Sub Setup()
    HookChart
    AddFxButton
    MsgBox "설정을 마쳤습니다." & vbCrLf & vbCrLf & _
           "· 대시보드 차트의 막대를 더블클릭하면 상세 시트로 이동합니다." & vbCrLf & _
           "· 환율·금리 시트의 버튼으로 매매기준율을 가져옵니다.", vbInformation, "자금 지급일정 대시보드"
End Sub

Public Sub HookChart()
    On Error GoTo Fail
    Set gChart = New clsChartEvents
    Set gChart.Cht = ThisWorkbook.Worksheets(SH_DASH).ChartObjects(1).Chart
    Exit Sub
Fail:
    Set gChart = Nothing
End Sub

Private Sub AddFxButton()
    Dim ws As Worksheet, b As Object
    Set ws = ThisWorkbook.Worksheets(SH_FX)
    On Error Resume Next
    ws.Buttons("btnFetchFx").Delete
    On Error GoTo 0
    ws.Range("H7").ClearContents
    With ws.Range("H8:K10")
        Set b = ws.Buttons.Add(.Left, .Top, .Width, .Height)
    End With
    b.Name = "btnFetchFx"
    b.Caption = "서울외국환중개 매매기준율 가져오기"
    b.OnAction = "FetchFxRates"
End Sub

' 차트 더블클릭 → 상세 시트 (pointIndex: 1 = 첫 달)
Public Sub ShowDetail(ByVal pointIndex As Long, ByVal category As String)
    Dim ws As Worksheet
    Set ws = ThisWorkbook.Worksheets(SH_DETAIL)
    ws.Range("C4").Value = ThisWorkbook.Worksheets(SH_DASH).Cells(FIRST_MONTH_ROW + pointIndex - 1, 2).Value
    ws.Range("C5").Value = category
    ws.Activate
    ws.Range("B9").Select
End Sub

' ---------------------------------------------------------------- 환율 가져오기
Public Sub FetchFxRates()
    Dim ws As Worksheet, baseDt As Date, ccys As Variant, i As Long, r As Long
    Dim rate As Double, rateDt As Date, errMsg As String, okCount As Long, fails As String

    Set ws = ThisWorkbook.Worksheets(SH_FX)
    If Not IsDate(ws.Range("C4").Value) Then
        MsgBox "환율·금리 시트 C4에 보고 기준일을 날짜로 입력해 주세요.", vbExclamation
        Exit Sub
    End If
    baseDt = CDate(ws.Range("C4").Value)
    ccys = Array("USD", "EUR", "JPY")

    Application.StatusBar = "서울외국환중개에서 매매기준율을 가져오는 중..."
    For i = 0 To 2
        r = 10 + i                                  ' USD 10행, EUR 11행, JPY 12행
        If GetSmbsRate(CStr(ccys(i)), baseDt, rate, rateDt, errMsg) Then
            ws.Cells(r, 4).Value = rate             ' D열 매매기준율
            ws.Cells(r, 6).Value = rateDt           ' F열 고시일
            ws.Cells(r, 6).NumberFormat = "yyyy-mm-dd"
            okCount = okCount + 1
        Else
            fails = fails & ccys(i) & "(" & errMsg & ") "
        End If
    Next i
    Application.StatusBar = False

    ws.Range("C13").Value = Now
    If okCount = 3 Then
        ws.Range("C14").Value = "서울외국환중개 매매기준율 " & Format(baseDt, "yyyy-mm-dd") & " 기준 3개 통화 갱신 완료"
    Else
        ws.Range("C14").Value = "일부 실패: " & fails & "- 실패한 통화는 D열에 직접 입력하세요."
    End If
    Application.Calculate

    If okCount = 3 Then
        MsgBox "매매기준율을 가져왔습니다." & vbCrLf & _
               "USD " & Format(ws.Range("D10").Value, "#,##0.00") & " / EUR " & Format(ws.Range("D11").Value, "#,##0.00") & _
               " / JPY(100엔) " & Format(ws.Range("D12").Value, "#,##0.00"), vbInformation
    Else
        MsgBox "일부 통화를 가져오지 못했습니다: " & fails & vbCrLf & _
               "회사 네트워크에서 www.smbs.biz 접속이 막혀 있을 수 있습니다. D열에 직접 입력해도 됩니다.", vbExclamation
    End If
End Sub

' 기준일 이전 14일 구간을 조회해 기준일 이하 가장 최근 고시값을 돌려준다 (휴일이면 직전 영업일).
Private Function GetSmbsRate(ByVal ccy As String, ByVal baseDt As Date, ByRef rate As Double, _
                             ByRef rateDt As Date, ByRef errMsg As String) As Boolean
    Dim http As Object, body As String, url As String
    Dim re As Object, m As Object, d As Date, v As Double, found As Boolean

    url = SMBS_URL & ccy & "_" & Format(baseDt - 14, "yyyy-mm-dd") & "_" & Format(baseDt, "yyyy-mm-dd")
    On Error GoTo NetFail
    Set http = CreateObject("MSXML2.XMLHTTP")
    http.Open "GET", url, False
    http.setRequestHeader "Cache-Control", "no-cache"
    http.send
    If http.Status <> 200 Then
        errMsg = "HTTP " & http.Status
        Exit Function
    End If
    body = http.responseText
    On Error GoTo 0

    Set re = CreateObject("VBScript.RegExp")
    re.Global = True
    re.IgnoreCase = True
    re.Pattern = "<set\b[^>]*>"
    For Each m In re.Execute(body)
        If ParseSet(CStr(m.Value), d, v) Then
            If d <= baseDt And (Not found Or d > rateDt) Then
                rateDt = d
                rate = v
                found = True
            End If
        End If
    Next m

    If Not found Then
        errMsg = "자료 없음"
        Exit Function
    End If
    If ccy = "JPY" And rate < 50 Then rate = rate * 100   ' 1엔 단위로 온 경우 100엔 단위로 맞춤
    GetSmbsRate = True
    Exit Function
NetFail:
    errMsg = "접속 실패"
End Function

' <set label="26.11.13" value="1,385.00" ...> 형태에서 날짜와 값을 읽는다.
Private Function ParseSet(ByVal tag As String, ByRef d As Date, ByRef v As Double) As Boolean
    Dim lab As String, sVal As String, p As Variant, y As Long
    lab = AttrValue(tag, "label")
    sVal = Replace(AttrValue(tag, "value"), ",", "")
    If lab = "" Or sVal = "" Then Exit Function
    p = Split(NormalizeDate(lab), "-")
    If UBound(p) <> 2 Then Exit Function
    y = CLng(p(0))
    If y < 100 Then y = y + 2000
    d = DateSerial(y, CLng(p(1)), CLng(p(2)))
    v = Val(sVal)
    ParseSet = (v > 0)
End Function

Private Function AttrValue(ByVal tag As String, ByVal attrName As String) As String
    Dim re As Object, ms As Object
    Set re = CreateObject("VBScript.RegExp")
    re.IgnoreCase = True
    re.Pattern = attrName & "\s*=\s*[""']([^""']*)[""']"
    Set ms = re.Execute(tag)
    If ms.Count > 0 Then AttrValue = Trim$(ms(0).SubMatches(0))
End Function

Private Function NormalizeDate(ByVal s As String) As String
    Dim re As Object
    Set re = CreateObject("VBScript.RegExp")
    re.Global = True
    re.Pattern = "[^0-9]+"
    s = Trim$(re.Replace(s, "-"))
    If Left$(s, 1) = "-" Then s = Mid$(s, 2)
    If Right$(s, 1) = "-" Then s = Left$(s, Len(s) - 1)
    If Len(s) = 8 And InStr(s, "-") = 0 Then s = Left$(s, 4) & "-" & Mid$(s, 5, 2) & "-" & Right$(s, 2)
    NormalizeDate = s
End Function
