let Account
    : Type
    = { name : Text
      , address : Text
      , port : Natural
      , username : Text
      , password_command : Text
      , notification_command : Optional Text
      }

let gmail
    : Account
    = { name = "gmail"
      , address = "imap.gmail.com"
      , port = 993
      , username = "example@gmail.com"
      , password_command = "secret-tool lookup id gmail-account"
      , notification_command = None Text
      }

in  [ gmail ]
