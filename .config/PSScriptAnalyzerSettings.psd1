@{
    Severity = @('Error', 'Warning')
    IncludeRules = @(
        'PSAvoidAssignmentToAutomaticVariable'
        'PSAvoidUsingConvertToSecureStringWithPlainText'
        'PSAvoidUsingInvokeExpression'
        'PSAvoidUsingPlainTextForPassword'
        'PSUseDeclaredVarsMoreThanAssignments'
    )
}
