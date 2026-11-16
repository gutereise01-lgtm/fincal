VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
END
Attribute VB_Name = "clsChartEvents"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' 대시보드 차트 이벤트: 막대를 더블클릭하면 해당 월·구분의 상세 내역으로 이동

Public WithEvents Cht As Chart
Private mX As Long, mY As Long

Private Sub Cht_MouseDown(ByVal Button As Long, ByVal Shift As Long, ByVal x As Long, ByVal y As Long)
    mX = x
    mY = y
End Sub

Private Sub Cht_BeforeDoubleClick(ByVal ElementID As Long, ByVal Arg1 As Long, ByVal Arg2 As Long, Cancel As Boolean)
    Dim id As Long, a1 As Long, a2 As Long
    On Error Resume Next
    Cht.GetChartElement mX, mY, id, a1, a2
    On Error GoTo 0
    If id = xlSeries And a2 > 0 Then
        Cancel = True
        ShowDetail a2, Cht.SeriesCollection(a1).Name
    ElseIf ElementID = xlSeries And Arg2 > 0 Then
        Cancel = True
        ShowDetail Arg2, Cht.SeriesCollection(Arg1).Name
    End If
End Sub
