-- Nosferatu 1922 (dea TEE, Original 2023) - Lua port of the VBScript ruleset,
-- driving the Rust FlexDMD (see the engine's `flexdmd` module).
--
-- Ported incrementally. This file currently covers the spine: the DMD text
-- engine, score display, ball lifecycle and base scoring. Feature modes
-- (BLOOD IS LIFE, NOSFERATU, PLAGUE, sailors, ...) are being layered on.

-- ===========================================================================
-- VBScript string helpers
-- ===========================================================================
local function Space(n) return string.rep(" ", math.max(0, n)) end
local function Len(s) return #s end
local function Left(s, n) return string.sub(s, 1, n) end
local function Right(s, n) return string.sub(s, #s - n + 1) end
local function Mid(s, i, n) return string.sub(s, i, i + (n or (#s)) - 1) end
local function Asc(s) return string.byte(s) or 32 end

-- ===========================================================================
-- Constants / globals
-- ===========================================================================
local MaxPlayers, BallsPerGame = 1, 3
local eNone, eScrollLeft, eScrollRight, eBlink, eBlinkFast = 0, 1, 2, 3, 4
local dqSize = 64

Score, BonusPoints = {}, {}
PlayfieldMultiplier, BallsRemaining, ExtraBallsAwards = {}, {}, {}
CurrentPlayer, PlayersPlayingGame, Credits = 1, 1, 0
bGameInPlay, bOnTheFirstBall, Tilt, Tilted = false, false, 0, false
BallsOnPlayfield, LockedBalls = 0, 0
bMultiBallMode, bAutoPlunger, bBallSaverActive = false, false, false
BloodisLife, KickerJacks, bbilMB, bNOSFER = false, false, false, false
TotalGamesPlayed = 0
HighScore, HighScoreName = {}, {}
local bFreePlay = true
local myversion = "1.00"

-- DMD queue / effect state
local dqHead, dqTail = 0, 0
local deSpeed, deBlinkSlowRate, deBlinkFastRate = 20, 10, 5
local dLine = { [0] = "", [1] = "", [2] = " " }
local deCount, deCountEnd, deBlinkCycle = { [0] = 0, [1] = 0, [2] = 0 }, { [0] = 0, [1] = 0, [2] = 0 }, { [0] = 0, [1] = 0, [2] = 0 }
local dqText, dqEffect = {}, {}
local dqTimeOn, dqbFlush, dqSound = {}, {}, {}
local Chars = {}

FlexDMD, DMDScene = nil, nil

-- ===========================================================================
-- Sound / music shims (VBScript helpers)
-- ===========================================================================
function PlaySound(name) if name and name ~= "" then playsound(name) end end
function PlaySoundAt(name, _obj) PlaySound(name) end
function SoundFXDOF(name, ...) return name end
function SoundFX(name, ...) return name end
function PlaySong(_name) end
function ChangeSong() end
function StopSong() end

-- ===========================================================================
-- Scheduler: emulate vpmtimer.addtimer using the PulseTimer vpx timer tick.
-- ===========================================================================
local SCHED_MS = 25
local scheduled = {}
function after(ms, fn) scheduled[#scheduled + 1] = { ms, fn } end
function pulsetimer_timer()
  local i = 1
  while i <= #scheduled do
    local s = scheduled[i]
    s[1] = s[1] - SCHED_MS
    if s[1] <= 0 then
      table.remove(scheduled, i)
      s[2]()
    else
      i = i + 1
    end
  end
end

-- ===========================================================================
-- DMD text engine (ported from the VBScript JP-style flasher/FlexDMD DMD)
-- ===========================================================================
local function DMDInit()
  for n = 0, 255 do Chars[n] = "d_empty" end
  Chars[32] = "d_empty"
  Chars[42] = "d_star"; Chars[43] = "d_plus"; Chars[45] = "d_minus"; Chars[46] = "d_dot"
  Chars[60] = "d_less"; Chars[62] = "d_more"
  for d = 0, 9 do Chars[48 + d] = "d_" .. d end
  for c = 0, 25 do Chars[65 + c] = "d_" .. string.char(97 + c) end -- A..Z -> d_a..d_z
end

-- The desktop DMD is a grid of `digit001..041` image flashers (vpinball drives
-- their images, not FlexDMD; the vpx positions them, left of the playfield).
-- `Digits(adigit)` = digit<adigit+1>; digit041 (index 40) is the back image.
local function digitFlasher(idx)
  return _G["digit" .. string.format("%03d", idx + 1)]
end

local function DMDDisplayChar(achar, adigit)
  if achar == "" then achar = " " end
  local glyph = Chars[Asc(achar)] or "d_empty"
  digitFlasher(adigit).ImageA = glyph
  -- Also feed the Rust FlexDMD (shown as the right backbox panel; this is the
  -- path vpinball uses in cabinet mode). Same content, image-cell DMD.
  if FlexDMD then DMDScene:GetImage("Dig" .. adigit).Bitmap = "VPX." .. glyph .. "&dmd=2&add" end
end

local function DMDUpdate(id)
  if id == 0 then
    for digit = 0, 19 do DMDDisplayChar(Mid(dLine[0], digit + 1, 1), digit) end
  elseif id == 1 then
    for digit = 20, 39 do DMDDisplayChar(Mid(dLine[1], digit - 19, 1), digit) end
  elseif id == 2 then
    if dLine[2] == "" or dLine[2] == " " then dLine[2] = "d_border" end
    digitFlasher(40).ImageA = dLine[2]
    if FlexDMD then DMDScene:GetImage("Back").Bitmap = "VPX." .. dLine[2] .. "&dmd=2" end
  end
end

function DMDFlush()
  DMDTimer.Enabled = false
  DMDEffectTimer.Enabled = false
  dqHead, dqTail = 0, 0
  for i = 0, 2 do deCount[i] = 0; deCountEnd[i] = 0; deBlinkCycle[i] = 0 end
end

local function ExpandLine(s)
  if s == "" then return Space(20) end
  if Len(s) > 20 then return Left(s, 20) end
  if Len(s) < 20 then return s .. Space(20 - Len(s)) end
  return s
end

function FormatScore(num)
  -- The original inserts thousands commas via +128 font glyphs the d_* set
  -- lacks, so render plain digits for now.
  return tostring(math.abs(math.floor(num)))
end

function FL(a, b)
  if Len(a) + Len(b) < 20 then return a .. Space(20 - Len(a) - Len(b)) .. b end
  return Left(a .. b, 20)
end

function CL(s)
  if Len(s) > 20 then s = Left(s, 20) end
  local t = math.floor((20 - Len(s)) / 2)
  return Space(t) .. s .. Space(t)
end

function RL(s)
  if Len(s) > 20 then s = Left(s, 20) end
  return Space(20 - Len(s)) .. s
end

local function DMDHead()
  deCount[0], deCount[1], deCount[2] = 0, 0, 0
  for i = 0, 2 do
    local e = dqEffect[i][dqHead]
    if e == eNone then
      deCountEnd[i] = 1
    elseif e == eScrollLeft or e == eScrollRight then
      deCountEnd[i] = Len(dqText[i][dqHead])
    elseif e == eBlink or e == eBlinkFast then
      deCountEnd[i] = math.floor(dqTimeOn[dqHead] / deSpeed)
      deBlinkCycle[i] = 0
    end
  end
  if dqSound[dqHead] ~= "" then PlaySound(dqSound[dqHead]) end
  DMDEffectTimer.Interval = deSpeed
  DMDEffectTimer.Enabled = true
end

function DMD(Text0, Text1, Text2, Effect0, Effect1, Effect2, TimeOn, bFlush, Sound)
  if dqTail >= dqSize then return end
  dqText[0] = dqText[0] or {}; dqText[1] = dqText[1] or {}; dqText[2] = dqText[2] or {}
  dqEffect[0] = dqEffect[0] or {}; dqEffect[1] = dqEffect[1] or {}; dqEffect[2] = dqEffect[2] or {}
  local function setline(i, txt, eff)
    if txt == "_" then dqEffect[i][dqTail] = eNone; dqText[i][dqTail] = "_"
    else dqEffect[i][dqTail] = eff; dqText[i][dqTail] = (i == 2) and txt or ExpandLine(txt) end
  end
  setline(0, Text0, Effect0)
  setline(1, Text1, Effect1)
  setline(2, Text2, Effect2)
  dqTimeOn[dqTail] = TimeOn
  dqbFlush[dqTail] = bFlush
  dqSound[dqTail] = Sound
  dqTail = dqTail + 1
  if dqTail == 1 then DMDHead() end
end

local function DMDScore()
  local tmp, tmp1, tmp2
  if dqHead == dqTail then
    tmp = RL(FormatScore(Score[CurrentPlayer] or 0))
    tmp1 = FL("PLAYER " .. CurrentPlayer, "BALL " .. Balls())
    tmp2 = "d_border"
  end
  DMD(tmp, tmp1, tmp2, eNone, eNone, eNone, 10, true, "")
end

function DMDScoreNow()
  DMDFlush()
  DMDScore()
end

function dmdeffecttimer_timer()
  DMDEffectTimer.Enabled = false
  -- DMDProcessEffectOn
  local BlinkEffect = false
  for i = 0, 2 do
    if deCount[i] ~= deCountEnd[i] then
      deCount[i] = deCount[i] + 1
      local e = dqEffect[i][dqHead]
      local Temp
      if e == eNone then
        Temp = dqText[i][dqHead]
      elseif e == eScrollLeft then
        Temp = Right(dLine[i], 19) .. Mid(dqText[i][dqHead], deCount[i], 1)
      elseif e == eScrollRight then
        Temp = Mid(dqText[i][dqHead], 21 - deCount[i], 1) .. Left(dLine[i], 19)
      elseif e == eBlink or e == eBlinkFast then
        BlinkEffect = true
        local rate = (e == eBlink) and deBlinkSlowRate or deBlinkFastRate
        if (deCount[i] % rate) == 0 then
          deBlinkCycle[i] = (deBlinkCycle[i] == 0) and 1 or 0
        end
        if deBlinkCycle[i] == 0 then Temp = dqText[i][dqHead]
        elseif i == 2 then Temp = "" else Temp = Space(20) end
      end
      if dqText[i][dqHead] ~= "_" then
        dLine[i] = Temp
        DMDUpdate(i)
      end
    end
  end
  if deCount[0] == deCountEnd[0] and deCount[1] == deCountEnd[1] and deCount[2] == deCountEnd[2] then
    if dqTimeOn[dqHead] == 0 then
      DMDFlush()
    else
      DMDTimer.Interval = BlinkEffect and 10 or dqTimeOn[dqHead]
      DMDTimer.Enabled = true
    end
  else
    DMDEffectTimer.Enabled = true
  end
end

function dmdtimer_timer()
  DMDTimer.Enabled = false
  local Head = dqHead
  dqHead = dqHead + 1
  if dqHead == dqTail then
    if dqbFlush[Head] then DMDScoreNow() else dqHead = 0; DMDHead() end
  else
    DMDHead()
  end
end

-- ===========================================================================
-- Scoring
-- ===========================================================================
function AddScore(points)
  if Tilted then return end
  Score[CurrentPlayer] = (Score[CurrentPlayer] or 0) + points * (PlayfieldMultiplier[CurrentPlayer] or 1)
end

function CheckMultiplier()
  local pm = 1
  if butl1.state == 1 then pm = 2 end
  if butl2.state == 1 then pm = 3 end
  if butl3.state == 1 then pm = 4 end
  if butl4.state == 1 then pm = 5 end
  if butl5.state == 1 then pm = 6 end
  if butl6.state == 1 then pm = 7 end
  PlayfieldMultiplier[CurrentPlayer] = pm
end

function SetPlayfieldMultiplier(n) PlayfieldMultiplier[CurrentPlayer] = n end

function Balls()
  local tmp = BallsPerGame - (BallsRemaining[CurrentPlayer] or BallsPerGame) + 1
  if tmp > BallsPerGame then return BallsPerGame end
  return tmp
end

function UpdateBallInPlay() end

-- ===========================================================================
-- High scores (persisted via the engine's key/value store -> .store.json)
-- ===========================================================================
local hsDefaults = { 5000000, 2500000, 1000000, 500000 }
local hsDefNames = { "AAA", "BBB", "CCC", "DDD" }
function SortHighscore()
  for _ = 0, 3 do
    for j = 0, 2 do
      if (HighScore[j] or 0) < (HighScore[j + 1] or 0) then
        HighScore[j], HighScore[j + 1] = HighScore[j + 1], HighScore[j]
        HighScoreName[j], HighScoreName[j + 1] = HighScoreName[j + 1], HighScoreName[j]
      end
    end
  end
  Savehs()
end
function Loadhs()
  for k = 0, 3 do
    local sv = store_get("HighScore" .. (k + 1))
    HighScore[k] = sv and tonumber(sv) or hsDefaults[k + 1]
    HighScoreName[k] = store_get("HighScore" .. (k + 1) .. "Name") or hsDefNames[k + 1]
  end
  local c = store_get("Credits"); Credits = c and tonumber(c) or 0
  local g = store_get("TotalGamesPlayed"); TotalGamesPlayed = g and tonumber(g) or 0
  SortHighscore()
end
function Savehs()
  for k = 0, 3 do
    store_set("HighScore" .. (k + 1), HighScore[k] or 0)
    store_set("HighScore" .. (k + 1) .. "Name", HighScoreName[k] or "AAA")
  end
  store_set("Credits", Credits)
  store_set("TotalGamesPlayed", TotalGamesPlayed)
end
function CheckHighScore()
  local tmp = Score[CurrentPlayer] or 0
  if tmp > (HighScore[0] or 0) then Credits = Credits + 1 end
  if tmp > (HighScore[3] or 0) then
    PlaySound("fx_Knocker")
    HighScore[3] = tmp
    HighScoreName[3] = "YOU" -- TODO: flipper initials entry
    SortHighscore()
  end
  EndOfBallComplete()
end

-- ===========================================================================
-- Attract
-- ===========================================================================
function ShowTableInfo()
  for p = 1, 4 do
    if (Score[p] or 0) ~= 0 then
      DMD(CL("LAST SCORE"), CL("PLAYER " .. p .. " " .. FormatScore(Score[p])), "", eNone, eNone, eNone, 3000, false, "")
    end
  end
  DMD("", CL("GAME OVER"), "", eNone, eBlink, eNone, 2000, false, "")
  if bFreePlay then
    DMD("", CL("FREE PLAY ONLY"), "", eNone, eBlink, eNone, 2000, false, "")
  else
    local msg = (Credits > 0) and "PRESS START" or "INSERT COIN"
    DMD(CL("CREDITS " .. Credits), CL(msg), "", eNone, eBlink, eNone, 2000, false, "")
  end
  DMD(CL("BASED ON THE NOVEL"), CL("DRACULA"), "", eNone, eNone, eNone, 3000, false, "")
  DMD(CL("BY"), CL("BRAM STOKER"), "", eScrollLeft, eScrollLeft, eNone, 2300, false, "")
  DMD("", "", "d_title", eNone, eNone, eNone, 5000, false, "")
  DMD("", CL("ROM VERSION " .. myversion), "", eNone, eNone, eNone, 1520, false, "")
  DMD(CL("HIGHSCORES"), Space(20), "", eScrollLeft, eScrollLeft, eNone, 20, false, "")
  DMD(CL("HIGHSCORES"), "", "", eBlinkFast, eNone, eNone, 1380, false, "")
  DMD(CL("HIGHSCORES"), "1> " .. HighScoreName[0] .. " " .. FormatScore(HighScore[0]), "", eNone, eScrollLeft, eNone, 2000, false, "")
  DMD("_", "2> " .. HighScoreName[1] .. " " .. FormatScore(HighScore[1]), "", eNone, eScrollLeft, eNone, 2000, false, "")
  DMD("_", "3> " .. HighScoreName[2] .. " " .. FormatScore(HighScore[2]), "", eNone, eScrollLeft, eNone, 2000, false, "")
  DMD("_", "4> " .. HighScoreName[3] .. " " .. FormatScore(HighScore[3]), "", eNone, eScrollLeft, eNone, 2000, false, "")
  DMD(Space(20), Space(20), "", eScrollLeft, eScrollLeft, eNone, 500, false, "")
end
function StartAttractMode()
  DMDFlush()
  ShowTableInfo()
  PlaySong("music_attract")
end
function StopAttractMode() DMDScoreNow() end

-- ===========================================================================
-- Multiball eject + movie overlays (movies are cosmetic, stubbed for now)
-- ===========================================================================
local MaxMultiballs = 5
mBalls2Eject = 0
bBallInPlungerLane = false
function PlayMovie(_movname) end
function StopMovie() end
-- Movie frame arrays referenced by feature awards (consumed by PlayMovie).
movcar, movell, movhut, movkno, movwer, movorl = {}, {}, {}, {}, {}, {}
movsai, movemp, movnos, movsun, movpla, movrat, movbil, movsuk = {}, {}, {}, {}, {}, {}, {}, {}

function AddMultiball(nballs)
  mBalls2Eject = mBalls2Eject + nballs
  CreateMultiballTimer.Interval = 800
  CreateMultiballTimer.Enabled = true
  createmultiballtimer_timer()
end
function createmultiballtimer_timer()
  if bBallInPlungerLane then return end
  if BallsOnPlayfield < MaxMultiballs then
    CreateNewBall()
    mBalls2Eject = mBalls2Eject - 1
    if mBalls2Eject <= 0 then CreateMultiballTimer.Enabled = false end
  else
    mBalls2Eject = 0
    CreateMultiballTimer.Enabled = false
  end
end

-- Plunger-lane tracking (used by the multiball eject + autoplunger).
function swplungerrest_hit() bBallInPlungerLane = true end
function swplungerrest_unhit()
  bBallInPlungerLane = false
  bBallSaverActive = true
end

-- ===========================================================================
-- Ball lifecycle
-- ===========================================================================
function ResetForNewGame()
  bGameInPlay = true
  StopAttractMode()
  PlaySound("cbell")
  TotalGamesPlayed = TotalGamesPlayed + 1
  CurrentPlayer = 1
  PlayersPlayingGame = 1
  bOnTheFirstBall = true
  for i = 1, MaxPlayers do
    Score[i] = 0; BonusPoints[i] = 0; PlayfieldMultiplier[i] = 1
    BallsRemaining[i] = BallsPerGame; ExtraBallsAwards[i] = 0
  end
  Tilt = 0
  UpdateBallInPlay()
  after(1500, FirstBall)
end

function FirstBall()
  ResetForNewPlayerBall()
  CreateNewBall()
end

function ResetForNewPlayerBall()
  SetPlayfieldMultiplier(1)
  bBallSaverReady = true
  PlaySong("music_gp")
  DMDScoreNow()
end

function CreateNewBall()
  BallRelease:createball()
  BallsOnPlayfield = BallsOnPlayfield + 1
  UpdateBallInPlay()
  PlaySoundAt("fx_Ballrel", BallRelease)
  BallRelease:kick(90, 4)
  if BloodisLife or KickerJacks then bMultiBallMode = true; bAutoPlunger = true end
end

function Drain_Hit()
  Drain:destroyball()
  SetPlayfieldMultiplier(1)
  for _, l in ipairs({ butl1, butl2, butl3, butl4, butl5, butl6 }) do l.state = 0 end
  if BallsOnPlayfield > 0 then BallsOnPlayfield = BallsOnPlayfield - 1 end
  PlaySoundAt("drain", Drain)
  if not bGameInPlay then return end
  if BallsOnPlayfield - LockedBalls == 0 then
    StopSong()
    UpdateBallInPlay()
    after(200, EndOfBall)
  end
end

function EndOfBall()
  DMDFlush()
  EndOfBall2()
end

function EndOfBall2()
  Tilt = 0
  if (ExtraBallsAwards[CurrentPlayer] or 0) > 0 then
    ExtraBallsAwards[CurrentPlayer] = ExtraBallsAwards[CurrentPlayer] - 1
    DMD(CL("EXTRA BALL"), CL("SHOOT AGAIN"), "", eNone, eBlink, eNone, 1500, true, "")
    ResetForNewPlayerBall()
    CreateNewBall()
  else
    BallsRemaining[CurrentPlayer] = (BallsRemaining[CurrentPlayer] or BallsPerGame) - 1
    if BallsRemaining[CurrentPlayer] <= 0 then
      CheckHighScore()
    else
      EndOfBallComplete()
    end
  end
end

function EndOfBallComplete()
  CurrentPlayer = CurrentPlayer -- single player
  DMDScoreNow()
  ResetForNewPlayerBall()
  CreateNewBall()
end

function EndOfGame()
  bGameInPlay = false
  PlaySound("s_GameOver")
  set_flippers_enabled(false)
  StartAttractMode()
end

-- ===========================================================================
-- Sun Phase: the playfield multiplier (SunTriggers stage it, SpinTrigger cashes)
-- ===========================================================================
local function sunLamps()
  return { butl1, butl2, butl3, butl4, butl5, butl6 }, { spbl1, spbl2, spbl3, spbl4, spbl5, spbl6 }
end
local function clearSunLamps()
  local b, s = sunLamps()
  for i = 1, 6 do b[i].state = 0; s[i].state = 0 end
end
local function sunTrigger(active)
  PlaySound("target")
  local b, s = sunLamps()
  for i = 1, 6 do
    b[i].state = (i == active) and 1 or 0
    s[i].state = (i == active) and 2 or 0
  end
end
-- Trigger -> multiplier-lamp mapping (from the VBScript).
function suntrigger004_hit() sunTrigger(1) end
function suntrigger001_hit() sunTrigger(2) end
function suntrigger002_hit() sunTrigger(3) end
function suntrigger003_hit() sunTrigger(4) end
function suntrigger005_hit() sunTrigger(5) end
function suntrigger006_hit() sunTrigger(6) end

function spinner001_spin()
  -- sound is played by the engine (sidecar spinners.hit).
  spblCOL001:duration(2, 150, 0)
end

function spintrigger001_hit()
  PlaySound("sunphase")
  PlayfieldMultiplier[CurrentPlayer] = 1
  local _, s = sunLamps()
  local awards = { 15000, 25000, 40000, 60000, 88000, 125000 }
  local labels = { "15.000", "25.000", "40.000", "60.000", "88.000", "125.000" }
  for i = 1, 6 do
    if s[i].state == 2 then
      AddScore(awards[i])
      DMDFlush()
      DMD(CL("SUN PHASE BONUS"), CL(labels[i]), "_", eNone, eBlinkFast, eNone, 1500, true, "")
      clearSunLamps()
      break
    end
  end
end

-- ===========================================================================
-- NOSF-RATU drop targets (light the letters; armed by kicker001 -> Enos later)
-- ===========================================================================
NosReady = false
function CheckNosReady()
  if lb1.state == 2 and lb2.state == 2 and lb3.state == 2 and lb4.state == 2
      and lb5.state == 2 and lb6.state == 2 and lb7.state == 2 and lb8.state == 2 then
    NosReady = true
  end
end
local function nosTarget(target, letter, lb)
  CheckMultiplier()
  AddScore(1000)
  target.IsDropped = true
  PlaySoundAt("fx_droptarget", target)
  letter.state = 1
  lb.state = 2
  CheckNosReady()
end
function target1n_hit() nosTarget(Target1N, Nnos, lb1) end
function target2o_hit() nosTarget(Target2O, Onos, lb2) end
function target3s_hit() nosTarget(Target3S, Snos, lb3) end
function target4f_hit() nosTarget(Target4F, Fnos, lb4) end
function target5r_hit() nosTarget(Target5R, Rnos, lb5) end
function target6a_hit() nosTarget(Target6A, Anos, lb6) end
function target7t_hit() nosTarget(Target7T, Tnos, lb7) end
function target8u_hit() nosTarget(Target8U, Unos, lb8) end

-- ===========================================================================
-- Inlanes / outlanes
-- ===========================================================================
function leftinlane_hit()
  PlayfieldMultiplier[CurrentPlayer] = 1
  PlaySound("sensor")
  if ILBLight1.state == 1 then
    AddScore(25000)
    DMDFlush()
    DMD(CL("DEAD FLOWERS 4 ELLEN"), CL("BONUS COLLECTED"), "_", eNone, eBlinkFast, eNone, 2000, true, "")
    clearSunLamps(); ILBLight1:duration(2, 2000, 0); PlaySoundAt("Bell10", LeftInlane)
  else
    AddScore(500); clearSunLamps()
  end
end

function rightinlane_hit()
  PlayfieldMultiplier[CurrentPlayer] = 1
  PlaySound("sensor")
  if ILBLight2.state == 1 then
    AddScore(25000)
    DMDFlush()
    DMD(CL("LAND OF SPECTRES"), CL("BONUS COLLECTED"), "_", eNone, eBlinkFast, eNone, 2000, true, "")
    clearSunLamps(); ILBLight2:duration(2, 2000, 0); PlaySoundAt("Bell10", RightInlane)
  else
    AddScore(500); clearSunLamps()
  end
end

local function outlane(obj)
  PlaySound("sensor"); PlaySound("ding1000"); PlaySoundAt("great", obj)
  PlayfieldMultiplier[CurrentPlayer] = 1
  AddScore(500)
  DMDFlush()
  DMD(CL("GREAT DEATH"), CL("...AND GOODBYE"), "_", eNone, eBlinkFast, eNone, 1500, true, "")
  clearSunLamps()
  gdr:duration(2, 1500, 0); gdl:duration(2, 1500, 0)
end
function leftoutlane_hit() outlane(LeftOutlane) end
function rightoutlane_hit() outlane(RightOutlane) end

-- ===========================================================================
-- A few base hit handlers (more features being ported)
-- ===========================================================================
-- Bumper / spinner sounds are played by the engine (sidecar); slingshots are not.
function bumper001_hit() AddScore(1000) end
function bumper002_hit() AddScore(1000) end
function bumper003_hit() AddScore(1000) end
function bumper004_hit() AddScore(1000) end
function leftslingshot_slingshot() AddScore(100); PlaySound("left_slingshot") end
function rightslingshot_slingshot() AddScore(100); PlaySound("right_slingshot") end

-- ===========================================================================
-- Init / input
-- ===========================================================================
function table_init()
  log("Nosferatu: table_init")
  set_flippers_enabled(true)

  -- DMD: the digit-grid image flashers are spawned by the engine at their vpx
  -- positions; we drive their images (vpinball's desktop/BGSet=0 path, measured).
  -- We ALSO build a FlexDMD with the same content so the Rust FlexDMD subsystem
  -- renders it as a separate panel (vpinball's cabinet/BGSet=1 path).
  FlexDMD = CreateObject("FlexDMD.FlexDMD")
  FlexDMD.RenderMode = 2
  FlexDMD.Width = 128
  FlexDMD.Height = 32
  FlexDMD.Clear = true
  FlexDMD.GameName = "nosferatu"
  FlexDMD.Run = true
  DMDScene = FlexDMD:NewGroup("Scene")
  DMDScene:AddActor(FlexDMD:NewImage("Back", "VPX.d_border"))
  DMDScene:GetImage("Back"):SetSize(FlexDMD.Width, FlexDMD.Height)
  for i = 0, 40 do DMDScene:AddActor(FlexDMD:NewImage("Dig" .. i, "VPX.d_empty&dmd=2")) end
  for i = 0, 19 do DMDScene:GetImage("Dig" .. i):SetBounds(4 + i * 6, 3, 6, 11) end
  for i = 20, 39 do DMDScene:GetImage("Dig" .. i):SetBounds(4 + (i - 20) * 6, 17, 6, 11) end
  FlexDMD.Stage:AddActor(DMDScene)

  DMDInit()
  DMDFlush()
  Loadhs()

  -- Start the scheduler tick.
  PulseTimer.Interval = SCHED_MS
  PulseTimer.Enabled = true

  StartAttractMode()
end

function table_keydown(code)
  if code == KeyAddCredit then
    Credits = Credits + 1
    PlaySound("fx_coin")
  elseif code == KeyStartGame then
    if not bGameInPlay then
      set_flippers_enabled(true)
      ResetForNewGame()
    end
  end
end
