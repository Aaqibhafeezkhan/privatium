<?--
This file is part of Privatium
apps/animals/views/teach.lsp
Summary: The learning form. No hx-* attribute, on purpose: a plain post that redirects, which
         the frame's swap navigation follows inside the document.
Notes: See README file for documentation and full license information.
--?>

<div class="animals">
<h1>I give up. What was it?</h1>

<? if err then ?>
  <p class="pv-error" role="alert"><?= icon('exclamation-triangle') ?> <?= err ?></p>
<? end ?>

<form method="post" action="<?= url('/teach') ?>">
  <?= csrf() ?>

  <label for="animal">Your animal</label>
  <input id="animal" name="animal" type="text" maxlength="40"
         placeholder="wombat" required autofocus>

  <label for="question">
    A yes/no question that tells it apart from <?= node and node.text or 'my guess' ?>
  </label>
  <input id="question" name="question" type="text" maxlength="120"
         placeholder="Does it have cubic droppings?" required
         aria-describedby="question-help">

  <?-- ALPINE. A hint, expanded on request. Nothing here is submitted, saved, or
       missed if the page reloads — the definition of the Alpine half. --?>
  <div id="question-help" x-data="disclosure">
    <p class="pv-help">Anything true for one animal and false for the other.</p>
    <button type="button" class="pv-btn pv-btn-quiet pv-js-only"
            x-on:click="toggle" x-bind:aria-expanded="open">
      <span x-text="label">Show</span> examples
    </button>
    <ul x-show="open" x-cloak>
      <li>Does it have feathers?</li>
      <li>Could it fit in a shoebox?</li>
      <li>Does it live in salt water?</li>
    </ul>
    <p class="pv-help" x-show="open" x-cloak>
      Avoid questions about what *you* think of it. "Is it scary?" splits the tree
      differently every time you play.
    </p>
  </div>

  <?-- Each radio is wrapped in its label AND named by `for`: the wrap gives a big
       click target, the `for` is what PV402 checks and what every assistive
       technology agrees on. --?>
  <fieldset>
    <legend>And for your animal, the answer is</legend>
    <label for="answer-yes">
      <input id="answer-yes" type="radio" name="answer" value="yes" checked> Yes
    </label>
    <label for="answer-no">
      <input id="answer-no" type="radio" name="answer" value="no"> No
    </label>
  </fieldset>

  <button type="submit" class="pv-btn pv-btn-primary">Teach me</button>
  <a class="pv-btn" href="<?= url('/') ?>">Cancel</a>
</form>
</div>
