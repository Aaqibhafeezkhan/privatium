<?--
This file is part of Privatium
apps/animals/views/_board.lsp
Summary: The game board, on its own so HTMX can swap it.
Notes: See README file for documentation and full license information.
--?>

<? if err then ?>
  <p class="pv-error" role="alert"><?= icon('exclamation-triangle') ?> <?= err ?></p>
<? end ?>

<? if won then ?>

  <p class="animals-win-mark" aria-hidden="true"><?= icon('trophy') ?></p>
  <h1>I guessed it!</h1>
  <p>You were thinking of a <?= node.text ?>. Want to play again?</p>
  <form method="post" action="<?= url('/start') ?>"
        hx-post="<?= url('/start') ?>" hx-target="#board">
    <?= csrf() ?>
    <button type="submit" class="pv-btn pv-btn-primary"><?= icon('arrow-counterclockwise') ?> Start over</button>
  </form>

<? elseif not node then ?>

  <?-- Only reachable if the starter tree could not be written; the board itself
       never plants anything, so offer the page that does. --?>
  <h1>I don't know any animals yet.</h1>
  <p><a class="pv-btn pv-btn-primary" href="<?= url('/') ?>">Start a game</a></p>

<? elseif node.kind == 'q' then ?>

  <h1><?= node.text ?></h1>
  <div class="pv-actions">
    <form method="post" action="<?= url('/answer') ?>"
          hx-post="<?= url('/answer') ?>" hx-target="#board">
      <?= csrf() ?><input type="hidden" name="choice" value="yes">
      <button type="submit" class="pv-btn pv-btn-primary">
        <?= icon('hand-thumbs-up') ?> Yes
      </button>
    </form>
    <form method="post" action="<?= url('/answer') ?>"
          hx-post="<?= url('/answer') ?>" hx-target="#board">
      <?= csrf() ?><input type="hidden" name="choice" value="no">
      <button type="submit" class="pv-btn">
        <?= icon('hand-thumbs-down') ?> No
      </button>
    </form>
  </div>

<? else ?>

  <h1>Is it a <?= node.text ?>?</h1>
  <div class="pv-actions">
    <form method="get" action="<?= url('/won') ?>"
          hx-get="<?= url('/won') ?>" hx-target="#board">
      <button type="submit" class="pv-btn pv-btn-primary">
        <?= icon('check-lg') ?> Yes — you got it
      </button>
    </form>
    <?-- A plain link, deliberately. Teaching is a different page with its own
         form and its own back button; swapping it into #board would break the
         browser's history for no gain. Not every interaction wants HTMX. --?>
    <a class="pv-btn" href="<?= url('/teach') ?>"><?= icon('x-lg') ?> No</a>
  </div>

<? end ?>

<? if stats then ?>
  <?-- One line, so the count reads as one sentence to a screen reader. --?>
  <p class="pv-meta"><?= stats.animals ?> animal<?= stats.animals == 1 and '' or 's' ?>, <?= stats.questions ?> question<?= stats.questions == 1 and '' or 's' ?></p>
<? end ?>
