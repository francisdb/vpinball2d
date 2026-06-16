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
StopMBmodes = false
CurrentLightEll, CurrentLightHut, CurrentLightKno, CurrentLightOrl = 0, 0, 0, 0
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

function drain_hit()
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
-- Top kicker (kicker001): BLOOD IS LIFE multiball, character bonus, NOSF arm
-- ===========================================================================
local function clearBilLamps()
  for _, l in ipairs({ fl1b, fl2l, fl3o, fl4o, fl5d, fl6i, fl7s, fl8l, fl9i, fl10f, fl11e }) do
    l.state = 0
  end
end

function CheckBil()
  if fl1b.state == 1 and fl2l.state == 1 and fl3o.state == 1 and fl4o.state == 1
      and fl5d.state == 1 and fl6i.state == 1 and fl7s.state == 1 and fl8l.state == 1
      and fl9i.state == 1 and fl10f.state == 1 and fl11e.state == 1 then
    BILl001.state = 2; BILl002.state = 2; kickL1.state = 2
    BloodisLife = true
  else
    BloodisLife = false
  end
end

-- Character (Thrall) meters: bumpers advance the active character (selected by
-- the EHKO lamps); a filled meter arms its Thrall lamp for a 15s collect window
-- at the top kicker (CheckCHARB). Order E,H,K,O matches the original collect.
local CHARS_ORDER = { "E", "H", "K", "O" }
local CHARS = {
  E = { lamps = { ell1, ell2, ell3, ell4, ell5 }, thrall = ThrallE1, movie = movell, label = "ELLEN " },
  H = { lamps = { hut1, hut2, hut3, hut4, hut5, hut6 }, thrall = ThrallH1001, movie = movhut, label = "HUTTER" },
  K = { lamps = { kno1, kno2, kno3, kno4, kno5 }, thrall = ThrallK1, movie = movkno, label = "KNOCK " },
  O = { lamps = { orl1, orl2, orl3, orl4, orl5, orl6 }, thrall = ThrallO1, movie = movorl, label = "ORLOCK" },
}
for _, c in pairs(CHARS) do c.cur = 1; c.total = #c.lamps end

local function lightUpChar(key)
  local c = CHARS[key]
  for _, l in ipairs(c.lamps) do l.state = 0 end
  for i = 1, math.min(c.cur, c.total) do c.lamps[i].state = 1 end
  c.cur = c.cur + 1
  if c.cur > c.total then
    for _, l in ipairs(c.lamps) do l.state = 2 end
    c.thrall.state = 1
    saucl1:duration(2, 15000, 0)
    after(15000, function() -- ThrallX1 15s expiry (gameitem timers aren't fired here)
      if c.thrall.state == 1 then
        c.thrall.state = 0
        for _, l in ipairs(c.lamps) do l.state = 0 end
        PlaySound("chareset")
        c.cur = 1
      end
    end)
  end
end

local function charProjector(movie)
  PlaySound("charb")
  PlaySoundAt("projector", Peg5)
  Light005R:duration(2, 1772, 0); Light005L:duration(2, 1772, 0)
  progR:duration(2, 1772, 0); progL:duration(2, 1772, 0)
  PlaySoundAt("projector", Peg)
  PlayMovie(movie)
  CheckMultiplier()
  AddScore(10000)
  DMDFlush()
end

function CheckCHARB()
  for _, k in ipairs(CHARS_ORDER) do
    local c = CHARS[k]
    if c.thrall.state == 1 then
      c.thrall.state = 0
      for _, l in ipairs(c.lamps) do l.state = 0 end
      saucl1.state = 0; c.cur = 1
      charProjector(c.movie)
      DMD(CL("CHARACTER BONUS"), CL(c.label .. " MULT X 10.000"), "_", eNone, eBlinkFast, eNone, 1250, true, "")
      return
    end
  end
end

local function ejectKicker001()
  -- replaces kicker001.TimerEnabled (kicker built-in timers aren't fired here)
  after(800, function()
    kicker001:kick(135, 70); PlaySoundAt("fx_kicker", kicker001)
  end)
end

function kicker001_hit()
  CheckBil()
  if BloodisLife then
    bbilMB = true; AddMultiball(1); bAutoPlunger = true
    Light004:duration(2, 2000, 0)
    CheckMultiplier(); AddScore(100000)
    DMDFlush()
    DMD(CL("BLOOD IS LIFE"), CL("MULTIBALL"), "_", eNone, eBlinkFast, eNone, 3000, true, "")
    sqL5.state = 2
    PlaySoundAt("UpperKickerEnter", kicker001); PlaySound("mbstarted")
    Enos.state = 1; EYELightR.state = 1; EYELightL.state = 1
    kickL1:duration(2, 1000, 0)
    BloodisLife = false; StopMBmodes = true
    BILl001.state = 0; BILl002.state = 0
    clearBilLamps()
    PlaySoundAt("projector", Peg5)
    Light005R:duration(2, 1772, 0); Light005L:duration(2, 1772, 0)
    progR:duration(2, 1772, 0); progL:duration(2, 1772, 0)
    PlaySoundAt("projector", Peg); PlayMovie(movbil)
  end
  CheckCHARB()
  PlaySoundAt("UpperKickerEnter", kicker001)
  Enos.state = 1; EYELightR.state = 1; EYELightL.state = 1
  if not KickerJacks then CheckNosReady() end
  kickL1:duration(2, 1000, 0)
  ejectKicker001()
end

-- ===========================================================================
-- NOSF-RATU drop targets (light the letters; collected when armed by kicker001)
-- ===========================================================================
NosReady = false
function CheckNosReady()
  if Enos.state == 1
      and Target1N.IsDropped and Target2O.IsDropped and Target3S.IsDropped and Target4F.IsDropped
      and Target5R.IsDropped and Target6A.IsDropped and Target7T.IsDropped and Target8U.IsDropped then
    NosReady = true
    saucl2.state = 2
    for _, l in ipairs({ Enos, Nnos, Onos, Snos, Fnos, Rnos, Anos, Tnos, Unos }) do l.state = 2 end
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
-- Side scoops (kicker002 left / kicker003 right): blood-cell bonus, kicker
-- jackpots, and the NOSFERATU / PLAGUE mode starts.
-- ===========================================================================
PlagueReady, PlagueModeActive, NosHitCount, BallSaveDoors = false, false, 0, false

-- The Nosferatu drop-target prop animates a primitive (no-op in 2D); the
-- IsDropped shadow state is what the rules read.
local function DropNosferatu()
  NosTarget001.IsDropped = true; NosTargetW.IsDropped = true
end
local function RiseNosferatu()
  NosTarget001.IsDropped = false; NosTargetW.IsDropped = false
end

-- Ball-saver doors (timed re-drop) are deferred; for now just clear the flag.
local function BallSaveCountDown() BallSaveDoors = false end

local function projectorFlash()
  PlaySoundAt("projector", Peg5)
  Light005R:duration(2, 1772, 0); Light005L:duration(2, 1772, 0)
  progR:duration(2, 1772, 0); progL:duration(2, 1772, 0)
  PlaySoundAt("projector", Peg)
end

function StopNOS()
  DropNosferatu()
  Nosul001.state = 0
  KickerJacks = false; bNOSFER = false; bMultiBallMode = false; bAutoPlunger = false
  LCRL.state = 2; LskL.state = 0; RCRL.state = 2; RskL.state = 0
end

function NOSFERATU()
  AddScore(35000)
  DMDFlush()
  DMD(CL("NOSFERATU"), CL("KICKER JACKPOTS"), "_", eNone, eBlink, eNone, 2000, true, "")
  Light004:duration(2, 2000, 0)
  RiseNosferatu()
  Nosul001.state = 2
  PlaySoundAt("fx_droptarget", NosTargetP1)
  Enos.state = 1
  for _, l in ipairs({ Nnos, Onos, Snos, Fnos, Rnos, Anos, Tnos, Unos }) do l.state = 0 end
  BallSaveDoors = true; BallSaveCountDown()
  DoorLeft.IsDropped = true; DoorLeft001.IsDropped = false
  PlaySoundAt("fx_droptargetreset", DoorLeft001); LCRL.state = 1; LskL.state = 0
  DoorRight.IsDropped = true; DoorRight001.IsDropped = false
  PlaySoundAt("fx_droptargetreset", DoorRight001); RCRL.state = 1; RskL.state = 0
  KickerJacks = true; NosHitCount = 0
  projectorFlash(); PlayMovie(movnos)
end

function PlagueMode()
  PlagueReady = false; PlagueModeActive = true
  PlaySound("plaguevoice")
  projectorFlash(); PlayMovie(movpla)
  CheckMultiplier(); AddScore(35000)
  DMDFlush()
  DMD(CL("PLAGUE PLAGUE PLAGUE"), CL("SHOOT THE ORBIT"), "_", eNone, eBlink, eNone, 2000, true, "")
  saucl3.state = 0
  for _, l in ipairs({ FlatPL, FlatLL001, FlatAL, FlatGL, FlatUL001, FlatEL }) do l.state = 0 end
  PLagueStatic.IsDropped = false
  DoorLeft.IsDropped = false; DoorLeft001.IsDropped = true
  PlaySoundAt("fx_droptargetreset", DoorLeft); LCRL.state = 2; LskL.state = 0
  DoorRight.IsDropped = false; DoorRight001.IsDropped = true
  PlaySoundAt("fx_droptargetreset", DoorRight); RCRL.state = 2; RskL.state = 0
  for _, t in ipairs({ Target1N, Target2O, Target3S, Target4F }) do t.IsDropped = false end
  WWTarget001.IsDropped = true; wul001.state = 0
  for _, l in ipairs({ Nnos, Onos, Snos, Fnos, Enos, EYELightR, EYELightL }) do l.state = 0 end
  PlaySoundAt("fx_droptarget", Target3S)
  for _, l in ipairs({ lb1, lb2, lb3, lb4, sealite1, sealite2, sealite3, deadsl001 }) do l.state = 0 end
  for _, t in ipairs({ Target5R, Target6A, Target7T, Target8U }) do t.IsDropped = false end
  CMTarget001.IsDropped = true; cul001.state = 0
  for _, l in ipairs({ Rnos, Anos, Tnos, Unos }) do l.state = 0 end
  PlaySoundAt("fx_droptarget", Target6A)
  for _, l in ipairs({ lb5, lb6, lb7, lb8 }) do l.state = 0 end
  for _, p in ipairs({ prise001, prise002, prise003, prise004, prise005, prise006,
    prise007, prise008, prise009, prise010 }) do p.IsDropped = false end
  PlaySoundAt("whooshup", prise001); PlaySoundAt("fx_droptarget", prise004)
  PlagueCenterLight.state = 1
end

-- Clear the BLOODISLIFE letters and one side's NOSF letters + blood cells after
-- a side scoop collects (shared by both scoops, parameterised per side).
local function scoopBloodCellBonus(side)
  DMDFlush()
  DMD(CL("BLOOD CELL BONUS"), CL("COLLECTED"), "_", eNone, eBlinkFast, eNone, 1250, true, "")
  CheckMultiplier()
  for _, lb in ipairs(side.cells) do
    if lb.state == 2 then AddScore(2500) end
  end
  PlaySoundAt("BloodKickerEnter", side.kicker)
  PlaySoundAt("Score500", side.kicker)
  PlaySoundAt("MotorLeer", side.kicker)
  side.door.IsDropped = false
  PlaySoundAt("fx_droptargetreset", side.door)
  side.crl.state = 2; side.skl.state = 0
  for _, t in ipairs(side.targets) do t.IsDropped = false end
  side.wcm.IsDropped = true; side.wcmLamp.state = 0
  clearBilLamps()
  for _, pl in ipairs({ PegLight, PegLight001, PegLight002, PegLight003, PegLight004, PegLight005,
    PegLight006, PegLight007, PegLight008, PegLight009, PegLight010 }) do pl.state = 0 end
  for _, l in ipairs(side.letters) do l.state = 0 end
  Enos.state = 0; EYELightR.state = 0; EYELightL.state = 0
  PlaySoundAt("fx_droptarget", side.dropSound)
  for _, lb in ipairs(side.cells) do lb.state = 0 end
  BloodisLife = false; BILl001.state = 0; BILl002.state = 0; kickL1.state = 0
  saucl2.state = 0; NosReady = false
end

function kicker002_hit()
  if NosReady then
    bNOSFER = true; ChangeSong(); NOSFERATU(); AddMultiball(1); bAutoPlunger = true
    kickL2:duration(2, 2250, 0); PlaySoundAt("fx_kicker", kicker002)
    saucl2.state = 0; NosReady = false
  end
  if KickerJacks then
    CheckMultiplier(); AddScore(30000)
    DMDFlush()
    DMD(CL("NOSFERATU"), CL("KICKER JACKPOTS"), "_", eNone, eBlink, eNone, 1250, true, "")
    kickL2:duration(2, 1000, 0); PlaySoundAt("fx_kicker", kicker002)
  else
    -- left side: WW (werewolf) sub-target, NOSF letters Nnos..Fnos, cells lb1..4
    scoopBloodCellBonus({
      cells = { lb1, lb2, lb3, lb4 }, kicker = kicker002, door = DoorLeft,
      crl = LCRL, skl = LskL, targets = { Target1N, Target2O, Target3S, Target4F },
      wcm = WWTarget001, wcmLamp = wul001, letters = { Nnos, Onos, Snos, Fnos },
      dropSound = Target3S,
    })
    sealite1.state = 0; sealite2.state = 0; sealite3.state = 0; deadsl001.state = 0
  end
  after(1000, function() kicker002:kick(184, 10); PlaySoundAt("popper_ball", kicker002) end)
end

function kicker003_hit()
  if PlagueReady then PlagueMode(); kickL3:duration(2, 850, 0) end
  if KickerJacks then
    CheckMultiplier(); AddScore(30000)
    DMDFlush()
    DMD(CL("NOSFERATU"), CL("KICKER JACKPOTS"), "_", eNone, eBlink, eNone, 1250, true, "")
    kickL3:duration(2, 1000, 0); PlaySoundAt("fx_kicker", kicker003); PlaySound("jackpot")
    projectorFlash(); PlayMovie(movsuk)
  else
    -- right side: CM (carriage man) sub-target, NOSF letters Rnos..Unos, cells lb5..8
    scoopBloodCellBonus({
      cells = { lb5, lb6, lb7, lb8 }, kicker = kicker003, door = DoorRight,
      crl = RCRL, skl = RskL, targets = { Target5R, Target6A, Target7T, Target8U },
      wcm = CMTarget001, wcmLamp = cul001, letters = { Rnos, Anos, Tnos, Unos },
      dropSound = Target6A,
    })
    WWTarget001.IsDropped = true; wul001.state = 0
  end
  after(1000, function() kicker003:kick(49, 50); PlaySoundAt("fx_kicker", kicker003) end)
end

-- NOSFERATU jackpot: during the mode (KickerJacks), hit the rising Nos target
-- 7 times; each hit drops it (raised again 500ms later via CheckNOST), the 7th
-- destroys Nosferatu for 200000 and ends the mode.
function CheckNOST()
  if KickerJacks then
    RiseNosferatu()
    Nosul001.state = 2
    PlaySoundAt("nosunhit", NosTargetP1)
  end
end

function nostarget001_hit()
  if not KickerJacks then return end
  NosHitCount = NosHitCount + 1
  PlaySoundAt("noshit", NosTargetP1)
  Light005:duration(2, 90, 0)
  for _, l in ipairs({ Nnos, Onos, Snos, Fnos, Rnos, Anos, Tnos, Unos }) do l:duration(2, 875, 0) end
  DropNosferatu(); Nosul001.state = 0
  CheckMultiplier()
  if NosHitCount < 7 then
    AddScore(15000)
    DMDFlush()
    DMD(CL("GREAT SHOT"), CL((7 - NosHitCount) .. " MORE TO GO..."), "_", eBlink, eBlinkFast, eNone, 1500, true, "")
    after(500, CheckNOST)
  else
    AddScore(200000)
    DMDFlush()
    DMD(CL("GREAT SHOT"), CL("NOSEFRATU DESTROYED"), "_", eBlink, eBlinkFast, eNone, 1500, true, "")
    projectorFlash(); PlayMovie(movsun)
    sqL7.state = 2; PlaySound("cbell"); PlaySound("MotorLeer")
    StopNOS()
  end
end

-- PLAGUE orbit: the orbit trigger raises the plague target; hit it 3 times
-- (each raised again by the trigger), the 3rd opens the final target for 75000.
PlaHitCount = 0
function plaguetrigger001_hit()
  if not PlagueModeActive then return end
  CheckMultiplier(); AddScore(5000)
  DMDFlush()
  DMD(CL("PLAGUE PLAGUE PLAGUE"), CL("SHOOT THE TARGET"), "_", eBlink, eBlinkFast, eNone, 1500, true, "")
  PLagueT1.IsDropped = false
  PlaySoundAt("DropTarget_Up", PLagueT1); PlaySoundAt("plagtrigshov", PLagueT1)
  plag1.state = 1; plag2.state = 0; plul1.state = 2
end

function plaguet1_hit()
  if not PlagueModeActive then return end
  PlaHitCount = PlaHitCount + 1
  CheckMultiplier(); AddScore(15000)
  PLagueT1.IsDropped = true
  PlaySoundAt("fx_droptarget", PLagueT1); PlaySoundAt("plaguecophit", PLagueT1)
  plag2.state = 2; plag1.state = 0; plul1.state = 0
  if PlaHitCount < 3 then
    DMDFlush()
    DMD(CL("GREAT SHOT"), CL("SHOOT THE ORBIT"), "_", eBlink, eBlinkFast, eNone, 1500, true, "")
    PLx1.state = 1
    if PlaHitCount == 2 then PLx2.state = 1 end
  else
    DMDFlush()
    DMD(CL("GREAT SHOT"), CL("KILL THE VAMPYRE"), "_", eBlink, eNone, eNone, 2000, true, "")
    PLx1.state = 1; PLx2.state = 1; PLx3.state = 1
    PLagueStatic.IsDropped = true; PlaySound("plaguevoice")
    for _, p in ipairs({ prise001, prise002, prise003, prise004, prise005, prise006,
      prise007, prise008, prise009, prise010 }) do p.IsDropped = true end
    PlaySoundAt("fx_droptarget", prise001); PlaySoundAt("Drop_Target_Down_2", prise004)
    PlaySoundAt("Drop_Target_Down_2", prise008)
    PlagueCenterLight.state = 0; PlagueModeActive = false
    FinalPLTarget001.IsDropped = false; FPTul001.state = 2
  end
end

function finalpltarget001_hit()
  CheckMultiplier(); AddScore(75000)
  DMDFlush()
  DMD(CL("PLAGUE BONUS"), CL("MODE COMPLETED"), "_", eNone, eBlinkFast, eNone, 2000, true, "")
  FinalPLTarget001.IsDropped = false; FPTul001.state = 0
  plag2.state = 0; PLx1.state = 0; PLx2.state = 0; PLx3.state = 0
  PlaHitCount = 0; sqL6.state = 1
  PlaySoundAt("Drop_Target_Down_2", FinalPLTarget001); PlaySound("MotorLeer"); PlaySound("plagwin")
end

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

-- Bumpers: 1000 + advance the active character meter (EHKO selects which one).
local function bumperHit()
  CheckMultiplier()
  AddScore(1000)
  if EHKO1.state == 1 then
    lightUpChar("E")
  elseif EHKO2.state == 1 then
    lightUpChar("H")
  elseif EHKO3.state == 1 then
    lightUpChar("K")
  elseif EHKO4.state == 1 then
    lightUpChar("O")
  end
end
function bumper001_hit() bumperHit() end
function bumper002_hit() bumperHit() end
function bumper003_hit() bumperHit() end
function bumper004_hit() bumperHit() end

-- Main slingshots cycle the active character lamp (EHKO1..4); mini slings score only.
PLightCounter = 1
local function cycleEHKO()
  CheckMultiplier(); AddScore(100)
  EHKO1.state = 0; EHKO2.state = 0; EHKO3.state = 0; EHKO4.state = 0
  local sel = { EHKO1, EHKO2, EHKO3, EHKO4 }
  sel[PLightCounter].state = 1
  PLightCounter = PLightCounter % 4 + 1
end
function wall001_slingshot() PlaySound("Chime_Right"); PlaySound("metalhit_medium"); cycleEHKO() end
function wall002_slingshot() PlaySound("Chime_Right"); PlaySound("metalhit_medium"); cycleEHKO() end
function wall003_slingshot() PlaySound("minislingL"); CheckMultiplier(); AddScore(100) end
function wall004_slingshot() PlaySound("minislingR"); CheckMultiplier(); AddScore(100) end
function wall006_slingshot() PlaySound("minislingR"); CheckMultiplier(); AddScore(100) end
function leftslingshot_slingshot() AddScore(100); PlaySound("left_slingshot") end
function rightslingshot_slingshot() AddScore(100); PlaySound("right_slingshot") end

-- ===========================================================================
-- BLOODISLIFE letters: pegs/targets light the 11 letters fl1b..fl11e; all 11
-- arm BloodisLife (CheckBil), collected at the top kicker for a multiball.
-- ===========================================================================
local function bloodLetter(fl, peglight, snd, src)
  if snd and src then PlaySoundAt(snd, src) end
  peglight.state = 2
  fl.state = 1
  CheckBil()
  if not BloodisLife then BILl001.state = 0; BILl002.state = 0 end
end

-- peg name -> { letter lamp, PegLight lamp, rubber sound }
local BLOOD_PEGS = {
  PegB001 = { fl1b, PegLight, "rubber_hit_1" }, PegB002 = { fl1b, PegLight, "rubber_hit_2" },
  PegB003 = { fl1b, PegLight, "rubber_hit_2" }, PegB004 = { fl1b, PegLight, "rubber_hit_3" },
  PegL001 = { fl2l, PegLight001, "rubber_hit_2" }, PegL002 = { fl2l, PegLight001, "rubber_hit_3" },
  PegL003 = { fl2l, PegLight001, "rubber_hit_1" }, PegL004 = { fl2l, PegLight001, "rubber_hit_3" },
  Pego1002 = { fl3o, PegLight002, "rubber_hit_2" }, Pego1003 = { fl3o, PegLight002, "rubber_hit_2" },
  Pego1004 = { fl3o, PegLight002, "rubber_hit_3" }, Pego1005 = { fl3o, PegLight002, "rubber_hit_1" },
  Pego2001 = { fl4o, PegLight003, "rubber_hit_1" }, Pego2002 = { fl4o, PegLight003, "rubber_hit_3" },
  Pego2003 = { fl4o, PegLight003, "rubber_hit_3" }, Pego2004 = { fl4o, PegLight003, "rubber_hit_2" },
  Pego2005001 = { fl4o, PegLight003, "rubber_hit_2" }, Pego2006 = { fl4o, PegLight003, "rubber_hit_2" },
  Pego2007 = { fl4o, PegLight003, "rubber_hit_2" }, Pego2008 = { fl4o, PegLight003, "rubber_hit_1" },
  PegD001 = { fl11e, PegLight004, "rubber_hit_3" }, PegD002 = { fl11e, PegLight004, "rubber_hit_2" },
  PegD003 = { fl11e, PegLight004, "rubber_hit_2" }, PegD004 = { fl11e, PegLight004, "rubber_hit_3" },
}
for name, def in pairs(BLOOD_PEGS) do
  _G[name:lower() .. "_hit"] = function() bloodLetter(def[1], def[2], def[3], _G[name]) end
end

-- D and I are the two mini-sling walls (they also fire <name>_slingshot above).
function wall003_hit() bloodLetter(fl5d, PegLight005, nil, nil) end
function wall004_hit() bloodLetter(fl6i, PegLight006, nil, nil) end

-- S, L, I, F are the under-the-playfield targets (2000 each).
local function bloodTarget(fl, peglight)
  CheckMultiplier(); AddScore(2000); PlaySound("target2")
  bloodLetter(fl, peglight, nil, nil)
end
function undertarg1_hit() bloodTarget(fl7s, PegLight007) end
function undertarg001_hit() bloodTarget(fl8l, PegLight008) end
function undertarg002_hit() bloodTarget(fl9i, PegLight009) end
function undertarg003_hit() bloodTarget(fl10f, PegLight010) end

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
