<?--
This file is part of Privatium
apps/_lint/fail/PV111/pv111bad/views/index.lsp
Summary: The one view, linking its own stylesheet and script, which a swapped page never loads.
Notes: See README file for documentation and full license information.
--?>

<link rel="stylesheet" href="<?= url('/static/app.css') ?>">
<script defer src="<?= url('/static/app.js') ?>"></script>

<h1>Lint</h1>
<p><a href="<?= url('/more') ?>">More</a></p>
