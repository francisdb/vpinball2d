-- Aztec (Williams 1976), game rules translated from the table's VBScript.
--
-- Pure game logic: scoring, bonus ladder, AZTEC letters, credits, players,
-- tilt, match and replays. Static feedback (flipper/slingshot/bumper/drain
-- sounds, slingshot animations) lives in the .table.json sidecar; the engine
-- plays those itself. DOF/B2S/desktop-backdrop code from the original is
-- dropped.
--
-- Install: copy this file and the .table.json next to the table's .vpx.

-- ---------------------------------------------------------------- state
local BALLS_PER_GAME = 5
local MAX_CREDITS = 25
local REPLAY = { 330000, 660000, 990000 }

local credits = 0
local in_game = false
local players = 0 -- players in the current game
local current = 1 -- player up (1..players)
local ball_in_play = 0
local score = { 0, 0, 0, 0 } -- wraps at 1M like the EM reels
local total = { 0, 0, 0, 0 } -- true score, for high score
local replays = { 0, 0, 0, 0 } -- replay level reached per player
local match_number = 0
local high3, high5 = 300250, 450250 -- high scores for 3- and 5-ball games

local tilted = false
local tilt_warnings = 0
local bonus = 1 -- bonus ladder position (1..10)
local letters = 0 -- completed AZTEC letter pairs (the "sm" multiplier)
local extra_lit = false -- shoot-again awarded this ball ("sa")
local game_over_pending = false -- end of game once the last bonus pays out ("eg")
local launch_armed = false
local reset_steps = 0
local draining = false

-- ---------------------------------------------------------------- items
local reel = { reel1, reel2, reel3, reel4 }
local player_no = { plno1, plno2, plno3, plno4 }
local player_up = { up1, up2, up3, up4 }
local match_box = { m0, m1, m2, m3, m4, m5, m6, m7, m8, m9 }
local ball_box = { bip1, bip2, bip3, bip4, bip5 }
local bonus_light = { b5k, b10k, b15k, b20k, b25k, b30k, b35k, b40k, b45k, b50k }
-- The five AZTEC letter targets: the white "letter open" light, and the two
-- awarded letter lights either side.
local letter_open = { upcl, midll, midrl, lowll, lowrl }
local letter_a = { al1, zl1, tl1, el1, cl1 }
local letter_b = { al2, zl2, tl2, el2, cl2 }

-- ---------------------------------------------------------------- helpers
local function gi_on()
    for _, l in ipairs(GI) do l.state = LightStateOn end
end

local function gi_off()
    for _, l in ipairs(GI) do l.state = LightStateOff end
end

local function chime(points)
    if points >= 10000 then playsound("bell1000low")
    elseif points >= 1000 then playsound("bell1000")
    elseif points >= 100 then playsound("bell100")
    else playsound("bell10") end
end

local function knocker_credit()
    if credits < MAX_CREDITS then credits = credits + 1 end
    playsound("knocker")
    playsound("click")
    credtxt.text = tostring(credits)
end

local function addscore(points)
    if tilted or not in_game then return end
    -- The match unit steps on every 10/100 score, like the EM stepper.
    if points == 10 or points == 100 then
        match_number = (match_number + 1) % 10
    end
    reel[current]:addvalue(points)
    chime(points)
    score[current] = score[current] + points
    total[current] = total[current] + points
    if score[current] >= 1000000 then
        score[current] = score[current] - 1000000
        replays[current] = 0
    end
    for level, threshold in ipairs(REPLAY) do
        if score[current] >= threshold and replays[current] == level - 1 then
            replays[current] = level
            shootagain.state = LightStateOn
            knocker_credit()
        end
    end
end

-- Bonus ladder up one step.
local function bonus_advance()
    bonus_light[bonus].state = LightStateOff
    bonus = math.min(bonus + 1, 10)
    bonus_light[bonus].state = LightStateOn
    playsound("clerker")
    playsound("bell1000")
end

-- AZTEC letter completion awards (shoot-again, specials, side lanes).
local function check_award()
    local a, z, t, e, c =
        letter_a[1].state, letter_a[2].state, letter_a[3].state,
        letter_a[4].state, letter_a[5].state
    local azt = a == 1 and z == 1 and t == 1
    local aec = a == 1 and e == 1 and c == 1
    if (azt or aec) and not extra_lit and shootagain.state == 0 then
        ebl.state = LightStateOn
        extra_lit = true
    end
    if (azt or aec) and bonus == 10 then
        sp1.state = LightStateOn
        sp2.state = LightStateOn
    end
    if z == 1 and t == 1 then
        leftsidel.state = LightStateOn
        lanekl.state = LightStateOn
    end
end

-- Alternate the lit bumper / top lane on every slingshot, the EM way.
local function alternate_lights()
    if uprl.state == 1 then
        bumper3l.state = 1
        bumper2l.state = 1
        bumper1l.state = 0
        uprl.state = 0
        upll.state = 1
    else
        bumper3l.state = 0
        bumper2l.state = 0
        bumper1l.state = 1
        uprl.state = 1
        upll.state = 0
    end
end

-- Per-ball light reset.
local function new_ball_lights()
    bonus = 1
    letters = 0
    bonus_light[1].state = LightStateOn
    for i = 2, 10 do bonus_light[i].state = LightStateOff end
    for i = 1, 5 do
        letter_open[i].state = LightStateOn
        letter_a[i].state = LightStateOff
        letter_b[i].state = LightStateOff
    end
    bumper3l.state = 0
    bumper2l.state = 0
    bumper1l.state = 1
    uprl.state = 1
    upll.state = 0
    topl.state = LightStateOff
    sp1.state = LightStateOff
    sp2.state = LightStateOff
    ebl.state = LightStateOff
    spinl.state = LightStateOff
    leftsidel.state = LightStateOff
    lanekl.state = LightStateOff
    dbl.state = LightStateOff
end

local function turnoff()
    for i = 1, 10 do bonus_light[i].state = LightStateOff end
    for i = 1, 5 do
        letter_open[i].state = LightStateOff
        letter_a[i].state = LightStateOff
        letter_b[i].state = LightStateOff
    end
    sp1.state = LightStateOff
    sp2.state = LightStateOff
    ebl.state = LightStateOff
    spinl.state = LightStateOff
    leftsidel.state = LightStateOff
    topl.state = LightStateOff
    lanekl.state = LightStateOff
    bumper1l.state = 0
    bumper2l.state = 0
    bumper3l.state = 0
    uprl.state = 0
    upll.state = 0
    dbl.state = LightStateOff
end

local function release_ball()
    nb:createball()
    nb:kick(135, 4)
    playsound("kickerkick")
    launch_armed = false
end

local function show_ball_in_play()
    for i = 1, 5 do
        ball_box[i].text = (i == ball_in_play) and tostring(i) or " "
    end
end

local function save_state()
    store_set("credits", credits)
    store_set("high3", high3)
    store_set("high5", high5)
    store_set("match", match_number)
end

-- Game-end match: a random-ish two-digit number; matching players win a credit.
local function match_award()
    for i = 1, 10 do match_box[i].text = " " end
    match_box[match_number + 1].text = string.format("%d0", match_number)
    for i = 1, players do
        if (match_number * 10) == (score[i] % 100) then
            knocker_credit()
        end
    end
end

local function end_game()
    match_award()
    in_game = false
    gamov.text = "GAME OVER"
    for i = 1, 4 do
        player_no[i].state = LightStateOff
        player_up[i].state = LightStateOff
    end
    for i = 1, players do
        if BALLS_PER_GAME == 3 and total[i] > high3 then high3 = total[i] end
        if BALLS_PER_GAME == 5 and total[i] > high5 then high5 = total[i] end
    end
    hstxt.text = tostring(BALLS_PER_GAME == 3 and high3 or high5)
    players = 0
    ball_in_play = 0
    for i = 1, 5 do ball_box[i].text = " " end
    playsound("motorleer")
    gi_off()
    save_state()
end

-- Advance to the next player/ball after a drain (bonus already paid).
local function next_ball()
    if tilted then
        tilted = false
        tilttxt.text = " "
        set_flippers_enabled(true)
    end
    if shootagain.state == 1 then
        -- Same player shoots again.
        extra_lit = false
        shootagain.state = LightStateOff
        new_ball_lights()
        release_ball()
        return
    end
    current = current + 1
    if current > players then
        current = 1
        ball_in_play = ball_in_play + 1
        if ball_in_play > BALLS_PER_GAME then
            end_game()
            return
        end
    end
    for i = 1, 4 do
        player_up[i].state = (i == current) and LightStateOn or LightStateOff
    end
    show_ball_in_play()
    new_ball_lights()
    release_ball()
end

-- ---------------------------------------------------------------- lifecycle
function table_init()
    credits = tonumber(store_get("credits")) or 0
    high3 = tonumber(store_get("high3")) or high3
    high5 = tonumber(store_get("high5")) or high5
    match_number = tonumber(store_get("match")) or 0
    credtxt.text = tostring(credits)
    ballstxt.text = tostring(BALLS_PER_GAME)
    hstxt.text = tostring(BALLS_PER_GAME == 3 and high3 or high5)
    gamov.text = "GAME OVER"
    turnoff()
    gi_off()
end

local function start_game()
    credits = credits - 1
    credtxt.text = tostring(credits)
    gi_on()
    playsound("click")
    playsound("initialize")
    players = 1
    current = 1
    player_no[1].state = LightStateOn
    player_up[1].state = LightStateOn
    game_over_pending = false
    ball_in_play = 1
    reset_steps = 0
    resettimer.enabled = true
end

local function add_player()
    credits = credits - 1
    credtxt.text = tostring(credits)
    player_no[players].state = LightStateOff
    players = players + 1
    player_no[players].state = LightStateOn
    playsound("click")
end

-- The EM score motor: tick the reels back to zero, then kick off the game.
function resettimer_timer()
    reset_steps = reset_steps + 1
    for i = 1, 4 do reel[i]:resettozero() end
    if reset_steps == 20 then playsound("kickerkick") end
    if reset_steps >= 24 then
        resettimer.enabled = false
        in_game = true
        for i = 1, 4 do
            score[i] = 0
            total[i] = 0
            replays[i] = 0
        end
        gamov.text = " "
        tilttxt.text = " "
        for i = 1, 10 do match_box[i].text = " " end
        show_ball_in_play()
        new_ball_lights()
        release_ball()
    end
end

-- ---------------------------------------------------------------- keys
function table_keydown(key)
    if key == KeyAddCredit then
        playsound("coin3")
        if credits < MAX_CREDITS then
            credits = credits + 1
            credtxt.text = tostring(credits)
            playsound("click")
        end
        save_state()
    elseif key == KeyStartGame then
        if credits > 0 and not in_game and players == 0 then
            start_game()
        elseif credits > 0 and in_game and players < 4 and ball_in_play < 2 then
            add_player()
        end
    elseif key == KeyTiltLeft or key == KeyTiltRight or key == KeyTiltCenter then
        check_tilt()
    end
end

function table_keyup(_) end

-- Nudge warnings within the tilt window add up; too many tilts the table.
function check_tilt()
    if not in_game or tilted then return end
    if tilttimer.enabled then
        tilt_warnings = tilt_warnings + 1
        if tilt_warnings >= 4 then
            tilted = true
            tilttxt.text = "TILT"
            playsound("tilt")
            turnoff()
            set_flippers_enabled(false)
            save_state()
        end
    else
        tilt_warnings = 0
        tilttimer.enabled = true
    end
end

function tilttimer_timer()
    tilttimer.enabled = false
end

-- ---------------------------------------------------------------- drain
function drain_hit()
    if not in_game then
        drain:destroyball()
        return
    end
    -- A second ball reaching the drain while the bonus is still paying out
    -- (e.g. one that was stuck and came loose) must not restart the payout.
    if draining then
        drain:destroyball()
        return
    end
    drain:destroyball()
    if tilted then
        next_ball()
    else
        draining = true
        bonuscount.enabled = true
    end
end

-- Pay the bonus ladder down, one step per tick, then release the next ball.
function bonuscount_timer()
    if bonus > 0 and bonus_light[bonus] ~= nil and not tilted then
        addscore(dbl.state == 1 and 10000 or 5000)
        bonus_light[bonus].state = LightStateOff
        bonus = bonus - 1
        if bonus > 0 then bonus_light[bonus].state = LightStateOn end
    end
    if bonus <= 0 then
        bonuscount.enabled = false
        draining = false
        next_ball()
    end
end

-- ---------------------------------------------------------------- playfield
function bumper1_hit()
    addscore(bumper1l.state == 1 and 1000 or 100)
end

function bumper2_hit()
    addscore(bumper2l.state == 1 and 1000 or 100)
end

function bumper3_hit()
    addscore(bumper3l.state == 1 and 1000 or 100)
end

function leftsling_slingshot()
    alternate_lights()
    addscore(10)
end

function rightsling_slingshot()
    alternate_lights()
    addscore(10)
end

function spinner1_spin()
    addscore(spinl.state == 1 and 1000 or 100)
end

-- Rollover buttons.
function rbut_hit()
    lbutt01.state = 1
    addscore(100)
end

function rbut_unhit() lbutt01.state = 0 end

function lbut_hit()
    lbutt03.state = 1
    addscore(100)
end

function lbut_unhit() lbutt03.state = 0 end

function cbut_hit()
    lbutt02.state = 1
    bonus_advance()
end

function cbut_unhit() lbutt02.state = 0 end

-- In/out lanes.
function leftin_hit() addscore(5000) end

function rightin_hit() addscore(5000) end

function leftout_hit()
    if sp1.state == 1 then
        shootagain.state = LightStateOn
        knocker_credit()
    else
        addscore(10000)
        bonus_advance()
    end
end

function rightout_hit()
    if sp2.state == 1 then
        shootagain.state = LightStateOn
        knocker_credit()
    else
        addscore(10000)
        bonus_advance()
    end
end

function leftside_hit()
    if leftsidel.state == 1 then dbl.state = LightStateOn end
    bonus_advance()
end

-- The lane kicker saucer: score, then kick the ball back out.
function lanek_hit()
    if lanekl.state == 1 then dbl.state = LightStateOn end
    if letters > 0 then
        addscore(10000 * letters)
    else
        addscore(1000)
    end
    playsound("kickerkick")
    kickertimer.enabled = true
end

function kickertimer_timer()
    lanek:kick(0, 18)
    kickertimer.enabled = false
end

-- AZTEC letter targets: an open (lit white) target awards its letter pair.
local function letter_target(index, follow_up)
    addscore(1000)
    if letter_open[index].state == 1 then
        letter_open[index].state = LightStateOff
        letter_a[index].state = LightStateOn
        letter_b[index].state = LightStateOn
        letters = letters + 1
        if follow_up then follow_up() end
    end
    check_award()
end

function upc_hit()
    spinl.state = LightStateOn
    letter_target(1)
end

function midl_hit()
    letter_target(2, function() uprl.state = 1 end)
end

function midr_hit()
    letter_target(3, function() topl.state = LightStateOn end)
end

function lowl_hit() letter_target(4) end

function lowr_hit() letter_target(5) end

function upl_hit()
    if upll.state == 1 then bonus_advance() end
    addscore(1000)
end

function upr_hit()
    if uprl.state == 1 then bonus_advance() end
    addscore(1000)
end

function top_hit()
    if topl.state == 1 then bonus_advance() end
    addscore(1000)
end

-- Centre target: collects the extra-ball light, scores by letters.
function midc_hit()
    if ebl.state == 1 then
        shootagain.state = LightStateOn
        ebl.state = LightStateOff
    end
    if letters > 0 then
        addscore(1000 * letters)
    else
        addscore(500)
    end
    bonus_advance()
end

-- Shooter lane: arm the launch sound at the ball's home, play it on the way out.
function ballhome_hit()
    launch_armed = true
end

function ballrel_hit()
    if launch_armed then
        playsound("launchball")
        launch_armed = false
    end
end
