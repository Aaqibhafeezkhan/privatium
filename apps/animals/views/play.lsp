<?--
This file is part of Privatium
apps/animals/views/play.lsp
Summary: The page around the board. The board itself is _board.lsp, because HTMX swaps it and a full
         page reload would lose the scroll position and the focus ring for no reason.
Notes: See README file for documentation and full license information.
--?>

<div class="animals">
<p class="animals-intro"><?= icon('diagram-3') ?> Think of an animal. Let's see if I can guess it.</p>
<div id="board">
  <?= render('_board', { node = node, stats = stats, err = err, won = won }) ?>
</div>

<footer class="pv-meta animals-links">
  <a href="<?= url('/knowledge') ?>"><?= icon('list-ul') ?> What I know</a>
  <form method="post" action="<?= url('/start') ?>"
        hx-post="<?= url('/start') ?>" hx-target="#board">
    <?= csrf() ?>
    <button type="submit" class="pv-btn pv-btn-quiet"><?= icon('arrow-counterclockwise') ?> Start over</button>
  </form>
</footer>
</div>
