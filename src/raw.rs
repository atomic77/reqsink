// FIXME Figure out how to get static js/html content built into the binary to avoid the need to have
// these files alongside the download. This is a copy/paste of the contents of templates/admin.html

pub const RAW_ADMIN_TEMPLATE: &str = r#"
<head>

    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/bootstrap@4.5.3/dist/css/bootstrap.min.css"
        integrity="sha384-TX8t27EcRE3e/ihU7zmQxVncDAy5uIKz4rEkgIXeMed4M0jlfIDPvg6uqKI2xXr2" crossorigin="anonymous">
    <!--    TODO Include this static content in build-->
    <script src="https://code.jquery.com/jquery-3.5.1.slim.min.js"
        integrity="sha384-DfXdz2htPH0lsSSs5nCTpuj/zy4C+OGpamoFVy38MVBnE+IbbVYUew+OrCXaRkfj"
        crossorigin="anonymous"></script>
    <script src="https://cdn.jsdelivr.net/npm/bootstrap@4.5.3/dist/js/bootstrap.bundle.min.js"
        integrity="sha384-ho+j7jyWK8fNQe+A12Hb8AhRq26LrZ/JpcUGGOn+Y7RsweNrtN/tE3MoK7ZeZDyx"
        crossorigin="anonymous"></script>
    <style>
        .bd-highlight {
            background-color: rgba(86, 61, 124, 0.15);
            border: 1px solid rgba(86, 61, 124, 0.15);
        }
    </style>
</head>

<body>

    <div class="jumbotron jumbotron-fluid p-3">
        <div class="container">
            <h3 class="display-4"> Welcome to reqsink </h3>
            <p class="lead">There have been {{ req_count }} tracked requests to the system.</p>
        </div>
    </div>

    <div class="container-fluid">
        {% for r in reqs %}

        <div class="row p-2">

            <div class="col-12 b-2 p-2 bd-highlight"> <strong> {{ r.method }} </strong> {{ r.path }} </div>
            <div class="col-6">
                <div class="card border-light">
                    <h5>Headers</h5>
                    <table class="table table-sm table-borderless">
                        <tbody>
                            {% for k, v in r.headers %}
                            <tr class="d-flex">
                                <td class="col-3"> {{ k }} </td>
                                <td class="col-6"> {{ v }}</td>
                            </tr>
                            {% endfor %}
                        </tbody>
                    </table>
                </div>
            </div>
            <div class="col-6">
                <div class="card">
                    <h5>Request Body</h5>
                    <pre><code>{{ r.body }}</code></pre>
                </div>
            </div>
            <div class="col-12 b-2 p-2 bd-highlight"> From {{ r.ip_addr }} at {{ r.time }} </div>
        </div>
    </div>

    {% endfor %}

    <a href="/admin?start={{ next_page }}" class="btn btn-primary">Next 10</a>

    </div>
</body>

"#;